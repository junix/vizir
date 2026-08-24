use std::collections::BTreeMap;

use vizir_core::{
    ChartMark, Color, FontWeight, GeometryNode, MirChart, MirDiagram, MirGeometry, MirScale,
    MirView, Origin, PathCommand, Point, Rect, ResolvedStyle, Scene2D, SceneNode, ShapeStyle,
    TextAnchor, Transform2D, VizError, VizMir, VizResult, map_linear,
};

use crate::layout::{LayeredLayoutProvider, LayoutProvider};

const INK: &str = "#1C2736";
const MUTED: &str = "#596579";
const GRID: &str = "#D7DEE8";
const BLUE: &str = "#3B6EF5";
const SURFACE: &str = "#F7F9FC";

pub fn build_scene(mir: &VizMir) -> VizResult<Scene2D> {
    let mut nodes = Vec::new();
    for view in &mir.views {
        nodes.push(match view {
            MirView::Chart(chart) => build_chart(chart)?,
            MirView::Diagram(diagram) => build_diagram(diagram)?,
            MirView::Geometry(geometry) => build_geometry(geometry)?,
        });
    }
    Ok(Scene2D {
        document_id: mir.document_id.clone(),
        width: mir.width,
        height: mir.height,
        background: mir.background.clone(),
        nodes,
        losses: mir.losses.clone(),
    })
}

fn build_chart(chart: &MirChart) -> VizResult<SceneNode> {
    let mut children = Vec::new();
    let plot = chart_plot_bounds(chart.frame);
    children.extend(build_grid_and_axes(chart, plot)?);
    match &chart.mark {
        ChartMark::Symbol {
            x_scale,
            y_scale,
            color_scale,
            size,
            items,
        } => {
            let x = linear_scale(chart, x_scale)?;
            let y = linear_scale(chart, y_scale)?;
            for item in items {
                let center = Point {
                    x: map_linear(item.x, x.0, x.1),
                    y: map_linear(item.y, y.0, y.1),
                };
                let color = resolve_color(
                    chart,
                    color_scale.as_deref(),
                    item.color_category.as_deref(),
                );
                children.push(SceneNode::Circle {
                    id: format!("{}/point/{}", chart.id, item.key),
                    bounds: Rect {
                        x: center.x - size,
                        y: center.y - size,
                        width: size * 2.0,
                        height: size * 2.0,
                    },
                    origin: Origin {
                        hir_node: chart.id.clone(),
                        data_key: Some(item.key.clone()),
                        generated_by: "build-symbol-scene".to_owned(),
                        explanation: format!(
                            "datum {} mapped through scales {} and {}",
                            item.key, x_scale, y_scale
                        ),
                    },
                    center,
                    radius: *size,
                    style: ResolvedStyle {
                        fill: color,
                        stroke: Color::hex("#FFFFFF"),
                        stroke_width: 1.5,
                        opacity: 0.9,
                    },
                });
            }
            children.extend(build_legend(chart, color_scale.as_deref()));
        }
        ChartMark::Line {
            x_scale,
            y_scale,
            color_scale,
            line_width,
            show_points,
            series,
        } => {
            let x = linear_scale(chart, x_scale)?;
            let y = linear_scale(chart, y_scale)?;
            for series in series {
                let color = resolve_color(
                    chart,
                    color_scale.as_deref(),
                    series.color_category.as_deref(),
                );
                let points = series
                    .points
                    .iter()
                    .map(|item| Point {
                        x: map_linear(item.x, x.0, x.1),
                        y: map_linear(item.y, y.0, y.1),
                    })
                    .collect::<Vec<_>>();
                let mut commands = Vec::new();
                for (index, point) in points.iter().enumerate() {
                    if index == 0 {
                        commands.push(PathCommand::Move { to: *point });
                    } else {
                        commands.push(PathCommand::Line { to: *point });
                    }
                }
                children.push(SceneNode::Path {
                    id: format!("{}/series/{}", chart.id, series.key),
                    bounds: bounds_for_points(&points),
                    origin: Origin {
                        hir_node: chart.id.clone(),
                        data_key: Some(series.key.clone()),
                        generated_by: "build-line-scene".to_owned(),
                        explanation: format!(
                            "series {} sorted by x and mapped through {} and {}",
                            series.key, x_scale, y_scale
                        ),
                    },
                    commands,
                    style: ResolvedStyle {
                        fill: Color::transparent(),
                        stroke: color.clone(),
                        stroke_width: *line_width,
                        opacity: 1.0,
                    },
                    marker_end: false,
                });
                if *show_points {
                    for (point, item) in points.iter().zip(&series.points) {
                        children.push(SceneNode::Circle {
                            id: format!("{}/point/{}", chart.id, item.key),
                            bounds: Rect {
                                x: point.x - 3.5,
                                y: point.y - 3.5,
                                width: 7.0,
                                height: 7.0,
                            },
                            origin: Origin {
                                hir_node: chart.id.clone(),
                                data_key: Some(item.key.clone()),
                                generated_by: "build-line-point-scene".to_owned(),
                                explanation: format!(
                                    "line datum {} retained for stable identity",
                                    item.key
                                ),
                            },
                            center: *point,
                            radius: 3.5,
                            style: ResolvedStyle {
                                fill: Color::hex("#FFFFFF"),
                                stroke: color.clone(),
                                stroke_width: 2.0,
                                opacity: 1.0,
                            },
                        });
                    }
                }
            }
            children.extend(build_legend(chart, color_scale.as_deref()));
        }
        ChartMark::Bar {
            category_scale,
            value_scale,
            color_scale,
            items,
        } => {
            let (categories, range, padding) = band_scale(chart, category_scale)?;
            let value = linear_scale(chart, value_scale)?;
            let baseline = map_linear(0.0, value.0, value.1);
            let step = (range[1] - range[0]) / categories.len().max(1) as f64;
            let width = step * (1.0 - padding);
            for item in items {
                let index = categories
                    .iter()
                    .position(|category| category == &item.category)
                    .ok_or_else(|| {
                        VizError::Diagnostic(format!(
                            "VIZ-SCENE-0001: category {:?} is missing from band scale",
                            item.category
                        ))
                    })?;
                let x = range[0] + step * index as f64 + (step - width) / 2.0;
                let y = map_linear(item.value, value.0, value.1);
                let top = y.min(baseline);
                let height = (baseline - y).abs();
                children.push(SceneNode::Rect {
                    id: format!("{}/bar/{}", chart.id, item.key),
                    bounds: Rect {
                        x,
                        y: top,
                        width,
                        height,
                    },
                    origin: Origin {
                        hir_node: chart.id.clone(),
                        data_key: Some(item.key.clone()),
                        generated_by: "build-bar-scene".to_owned(),
                        explanation: format!(
                            "bar {} mapped through {} and zero-preserving {}",
                            item.key, category_scale, value_scale
                        ),
                    },
                    radius: 5.0,
                    style: ResolvedStyle {
                        fill: resolve_color(
                            chart,
                            color_scale.as_deref(),
                            item.color_category.as_deref(),
                        ),
                        stroke: Color::transparent(),
                        stroke_width: 0.0,
                        opacity: 0.92,
                    },
                });
            }
            children.extend(build_legend(chart, color_scale.as_deref()));
        }
    }

    if let Some(title) = &chart.title {
        children.push(title_node(&chart.id, title, chart.frame));
    }

    Ok(SceneNode::Group {
        id: chart.id.clone(),
        bounds: frame_rect(chart.frame),
        origin: Origin {
            hir_node: chart.id.clone(),
            data_key: None,
            generated_by: "build-chart-scene".to_owned(),
            explanation: chart.provenance.join("; "),
        },
        transform: Transform2D::default(),
        opacity: 1.0,
        children,
    })
}

fn build_grid_and_axes(chart: &MirChart, plot: [f64; 4]) -> VizResult<Vec<SceneNode>> {
    let mut nodes = Vec::new();
    let x_scale = chart
        .scales
        .iter()
        .find(|scale| scale.id().ends_with("/x") || scale.id().ends_with("/category"));
    let y_scale = chart
        .scales
        .iter()
        .find(|scale| scale.id().ends_with("/y") || scale.id().ends_with("/value"));

    for index in 0..=5 {
        let fraction = index as f64 / 5.0;
        let y = plot[1] + (plot[3] - plot[1]) * fraction;
        nodes.push(line_node(
            format!("{}/grid/y/{index}", chart.id),
            Point { x: plot[0], y },
            Point { x: plot[2], y },
            Color::hex(GRID),
            1.0,
            0.7,
            &chart.id,
            "axis guide generated from normalized scale",
        ));
        if let Some(MirScale::Linear { domain, .. }) = y_scale {
            let value = domain[1] + (domain[0] - domain[1]) * fraction;
            nodes.push(text_node(
                format!("{}/axis/y/label/{index}", chart.id),
                Point {
                    x: plot[0] - 10.0,
                    y: y + 4.0,
                },
                format_number(value),
                11.0,
                TextAnchor::End,
                Color::hex(MUTED),
                FontWeight::Regular,
                &chart.id,
                "tick label generated from linear scale domain",
            ));
        }
    }
    if let Some(MirScale::Band { domain, range, .. }) = x_scale {
        let step = (range[1] - range[0]) / domain.len().max(1) as f64;
        let font_size = if domain.len() > 8 { 8.2 } else { 10.0 };
        for (index, category) in domain.iter().enumerate() {
            nodes.push(text_node(
                format!("{}/axis/x/category/{index}", chart.id),
                Point {
                    x: range[0] + step * (index as f64 + 0.5),
                    y: plot[3] + 20.0,
                },
                category.clone(),
                font_size,
                TextAnchor::Middle,
                Color::hex(MUTED),
                FontWeight::Regular,
                &chart.id,
                "category label generated from band scale domain",
            ));
        }
    }
    for index in 0..=5 {
        let fraction = index as f64 / 5.0;
        let x = plot[0] + (plot[2] - plot[0]) * fraction;
        nodes.push(line_node(
            format!("{}/grid/x/{index}", chart.id),
            Point { x, y: plot[1] },
            Point { x, y: plot[3] },
            Color::hex(GRID),
            1.0,
            0.45,
            &chart.id,
            "axis guide generated from normalized scale",
        ));
        if let Some(MirScale::Linear { domain, .. }) = x_scale {
            let value = domain[0] + (domain[1] - domain[0]) * fraction;
            nodes.push(text_node(
                format!("{}/axis/x/label/{index}", chart.id),
                Point {
                    x,
                    y: plot[3] + 20.0,
                },
                format_number(value),
                11.0,
                TextAnchor::Middle,
                Color::hex(MUTED),
                FontWeight::Regular,
                &chart.id,
                "tick label generated from linear scale domain",
            ));
        }
    }

    nodes.push(line_node(
        format!("{}/axis/x", chart.id),
        Point {
            x: plot[0],
            y: plot[3],
        },
        Point {
            x: plot[2],
            y: plot[3],
        },
        Color::hex(MUTED),
        1.4,
        1.0,
        &chart.id,
        "bottom axis emitted from explicit MIR guide",
    ));
    nodes.push(line_node(
        format!("{}/axis/y", chart.id),
        Point {
            x: plot[0],
            y: plot[1],
        },
        Point {
            x: plot[0],
            y: plot[3],
        },
        Color::hex(MUTED),
        1.4,
        1.0,
        &chart.id,
        "left axis emitted from explicit MIR guide",
    ));

    for guide in &chart.guides {
        if guide.kind != vizir_core::GuideKind::Axis {
            continue;
        }
        match guide.orient {
            vizir_core::GuideOrient::Bottom => nodes.push(text_node(
                format!("{}/axis/x/title", chart.id),
                Point {
                    x: (plot[0] + plot[2]) / 2.0,
                    y: chart.frame.y + chart.frame.height - 16.0,
                },
                guide.label.clone(),
                12.5,
                TextAnchor::Middle,
                Color::hex(INK),
                FontWeight::Medium,
                &chart.id,
                "axis title emitted from explicit MIR guide",
            )),
            vizir_core::GuideOrient::Left => nodes.push(text_node(
                format!("{}/axis/y/title", chart.id),
                Point {
                    x: chart.frame.x + 16.0,
                    y: plot[1] - 10.0,
                },
                guide.label.clone(),
                12.5,
                TextAnchor::Start,
                Color::hex(INK),
                FontWeight::Medium,
                &chart.id,
                "axis title emitted from explicit MIR guide",
            )),
            vizir_core::GuideOrient::Right => {}
        }
    }

    Ok(nodes)
}

fn build_legend(chart: &MirChart, scale_id: Option<&str>) -> Vec<SceneNode> {
    let Some(scale_id) = scale_id else {
        return Vec::new();
    };
    let Some(MirScale::OrdinalColor { domain, range, .. }) =
        chart.scales.iter().find(|scale| scale.id() == scale_id)
    else {
        return Vec::new();
    };
    let mut nodes = Vec::new();
    let columns = domain.len().clamp(1, 3);
    let start_x = chart.frame.x + chart.frame.width - (columns as f64 * 78.0 + 18.0);
    let start_y = chart.frame.y + 17.0;
    for (index, (label, color)) in domain.iter().zip(range).enumerate() {
        let x = start_x + (index % 3) as f64 * 78.0;
        let y = start_y + (index / 3) as f64 * 18.0;
        nodes.push(SceneNode::Circle {
            id: format!("{}/legend/{index}/swatch", chart.id),
            bounds: Rect {
                x,
                y: y - 5.0,
                width: 10.0,
                height: 10.0,
            },
            origin: Origin {
                hir_node: chart.id.clone(),
                data_key: None,
                generated_by: "build-legend".to_owned(),
                explanation: format!("legend swatch generated for category {label}"),
            },
            center: Point { x: x + 5.0, y },
            radius: 4.0,
            style: ResolvedStyle {
                fill: color.clone(),
                stroke: Color::transparent(),
                stroke_width: 0.0,
                opacity: 1.0,
            },
        });
        nodes.push(text_node(
            format!("{}/legend/{index}/label", chart.id),
            Point {
                x: x + 13.0,
                y: y + 4.0,
            },
            label.clone(),
            10.5,
            TextAnchor::Start,
            Color::hex(MUTED),
            FontWeight::Regular,
            &chart.id,
            "legend label generated from ordinal color scale",
        ));
    }
    nodes
}

fn build_diagram(diagram: &MirDiagram) -> VizResult<SceneNode> {
    let layout = LayeredLayoutProvider.layout(
        &diagram.layout_request.algorithm,
        diagram.frame,
        &diagram.nodes,
        &diagram.edges,
    )?;
    let node_width = 150.0;
    let node_height = 62.0;
    let mut children = Vec::new();

    for (edge_index, edge) in diagram.edges.iter().enumerate() {
        let from = *layout.positions.get(&edge.from).ok_or_else(|| {
            VizError::Diagnostic(format!(
                "VIZ-LAYOUT-0002: missing position for {:?}",
                edge.from
            ))
        })?;
        let to = *layout.positions.get(&edge.to).ok_or_else(|| {
            VizError::Diagnostic(format!(
                "VIZ-LAYOUT-0003: missing position for {:?}",
                edge.to
            ))
        })?;
        let delta_x = to.x - from.x;
        let delta_y = to.y - from.y;
        let (start, end, control1, control2) = if delta_x.abs() >= delta_y.abs() {
            let direction = delta_x.signum();
            let start = Point {
                x: from.x + direction * node_width / 2.0,
                y: from.y,
            };
            let end = Point {
                x: to.x - direction * node_width / 2.0,
                y: to.y,
            };
            let bend = ((end.x - start.x).abs() * 0.45).max(28.0);
            (
                start,
                end,
                Point {
                    x: start.x + direction * bend,
                    y: start.y,
                },
                Point {
                    x: end.x - direction * bend,
                    y: end.y,
                },
            )
        } else {
            let direction = delta_y.signum();
            let start = Point {
                x: from.x,
                y: from.y + direction * node_height / 2.0,
            };
            let end = Point {
                x: to.x,
                y: to.y - direction * node_height / 2.0,
            };
            let bend = ((end.y - start.y).abs() * 0.45).max(28.0);
            (
                start,
                end,
                Point {
                    x: start.x,
                    y: start.y + direction * bend,
                },
                Point {
                    x: end.x,
                    y: end.y - direction * bend,
                },
            )
        };
        let commands = vec![
            PathCommand::Move { to: start },
            PathCommand::Cubic {
                control1,
                control2,
                to: end,
            },
        ];
        children.push(SceneNode::Path {
            id: format!("{}/edge/{edge_index}-{}-{}", diagram.id, edge.from, edge.to),
            bounds: Rect::from_points(start, end),
            origin: Origin {
                hir_node: diagram.id.clone(),
                data_key: Some(format!("{}->{}", edge.from, edge.to)),
                generated_by: "route-diagram-edge".to_owned(),
                explanation: format!(
                    "edge {} -> {} routed after {}",
                    edge.from, edge.to, layout.explanation
                ),
            },
            commands,
            style: resolve_style(&edge.style, Color::transparent(), Color::hex("#8793A5")),
            marker_end: true,
        });
        if let Some(label) = &edge.label {
            children.push(text_node(
                format!("{}/edge/{edge_index}/label", diagram.id),
                Point {
                    x: (start.x + end.x) / 2.0,
                    y: (start.y + end.y) / 2.0 - 7.0,
                },
                label.clone(),
                10.5,
                TextAnchor::Middle,
                Color::hex(MUTED),
                FontWeight::Medium,
                &diagram.id,
                "edge label placed at resolved route midpoint",
            ));
        }
    }

    let group_palette = ["#EAF0FF", "#E5F7F3", "#FFF1E5", "#F0EBFF", "#FBE9F1"];
    let mut group_colors = BTreeMap::new();
    for node in &diagram.nodes {
        if let Some(group) = &node.group {
            let next = group_colors.len();
            group_colors
                .entry(group.clone())
                .or_insert_with(|| Color::hex(group_palette[next % group_palette.len()]));
        }
    }
    for node in &diagram.nodes {
        let center = layout.positions[&node.id];
        let bounds = Rect {
            x: center.x - node_width / 2.0,
            y: center.y - node_height / 2.0,
            width: node_width,
            height: node_height,
        };
        let default_fill = node
            .group
            .as_ref()
            .and_then(|group| group_colors.get(group))
            .cloned()
            .unwrap_or_else(|| Color::hex(SURFACE));
        children.push(SceneNode::Rect {
            id: format!("{}/node/{}/shape", diagram.id, node.id),
            bounds,
            origin: Origin {
                hir_node: node.id.clone(),
                data_key: Some(node.id.clone()),
                generated_by: "build-diagram-node".to_owned(),
                explanation: format!("node placed by {}; stable id preserved", layout.explanation),
            },
            radius: 14.0,
            style: resolve_style(&node.style, default_fill, Color::hex("#B8C3D3")),
        });
        children.push(text_node(
            format!("{}/node/{}/label", diagram.id, node.id),
            Point {
                x: center.x,
                y: center.y + 5.0,
            },
            node.label.clone(),
            if node.label.chars().count() > 22 {
                10.5
            } else {
                13.0
            },
            TextAnchor::Middle,
            Color::hex(INK),
            FontWeight::Medium,
            &node.id,
            "diagram label positioned inside resolved node bounds",
        ));
    }
    if let Some(title) = &diagram.title {
        children.push(title_node(&diagram.id, title, diagram.frame));
    }
    Ok(SceneNode::Group {
        id: diagram.id.clone(),
        bounds: frame_rect(diagram.frame),
        origin: Origin {
            hir_node: diagram.id.clone(),
            data_key: None,
            generated_by: "build-diagram-scene".to_owned(),
            explanation: format!("{}; {}", diagram.provenance.join("; "), layout.explanation),
        },
        transform: Transform2D::default(),
        opacity: 1.0,
        children,
    })
}

fn build_geometry(geometry: &MirGeometry) -> VizResult<SceneNode> {
    let mut children = geometry
        .children
        .iter()
        .map(|node| lower_geometry_node(node, &geometry.id))
        .collect::<Vec<_>>();
    if let Some(title) = &geometry.title {
        children.push(text_node(
            format!("{}/title", geometry.id),
            Point { x: 0.0, y: 24.0 },
            title.clone(),
            21.0,
            TextAnchor::Start,
            Color::hex(INK),
            FontWeight::Bold,
            &geometry.id,
            "geometry scene title retained from HIR",
        ));
    }
    Ok(SceneNode::Group {
        id: geometry.id.clone(),
        bounds: frame_rect(geometry.frame),
        origin: Origin {
            hir_node: geometry.id.clone(),
            data_key: None,
            generated_by: "build-geometry-scene".to_owned(),
            explanation: geometry.provenance.join("; "),
        },
        transform: Transform2D {
            translate: Point {
                x: geometry.frame.x,
                y: geometry.frame.y,
            },
            ..Transform2D::default()
        },
        opacity: 1.0,
        children,
    })
}

fn lower_geometry_node(node: &GeometryNode, owner: &str) -> SceneNode {
    match node {
        GeometryNode::Group {
            id,
            transform,
            opacity,
            children,
        } => SceneNode::Group {
            id: format!("{owner}/{id}"),
            bounds: geometry_group_bounds(children),
            origin: geometry_origin(owner, id, "lower-geometry-group"),
            transform: *transform,
            opacity: opacity.unwrap_or(1.0),
            children: children
                .iter()
                .map(|child| lower_geometry_node(child, owner))
                .collect(),
        },
        GeometryNode::Rect {
            id,
            x,
            y,
            width,
            height,
            radius,
            style,
        } => SceneNode::Rect {
            id: format!("{owner}/{id}"),
            bounds: Rect {
                x: *x,
                y: *y,
                width: *width,
                height: *height,
            },
            origin: geometry_origin(owner, id, "lower-geometry-rect"),
            radius: *radius,
            style: resolve_style(style, Color::transparent(), Color::transparent()),
        },
        GeometryNode::Circle {
            id,
            cx,
            cy,
            radius,
            style,
        } => SceneNode::Circle {
            id: format!("{owner}/{id}"),
            bounds: Rect {
                x: cx - radius,
                y: cy - radius,
                width: radius * 2.0,
                height: radius * 2.0,
            },
            origin: geometry_origin(owner, id, "lower-geometry-circle"),
            center: Point { x: *cx, y: *cy },
            radius: *radius,
            style: resolve_style(style, Color::transparent(), Color::transparent()),
        },
        GeometryNode::Line {
            id,
            from,
            to,
            style,
        } => SceneNode::Line {
            id: format!("{owner}/{id}"),
            bounds: Rect::from_points(*from, *to),
            origin: geometry_origin(owner, id, "lower-geometry-line"),
            from: *from,
            to: *to,
            style: resolve_style(style, Color::transparent(), Color::hex(INK)),
            marker_end: false,
        },
        GeometryNode::Path {
            id,
            commands,
            style,
        } => SceneNode::Path {
            id: format!("{owner}/{id}"),
            bounds: bounds_for_path(commands),
            origin: geometry_origin(owner, id, "lower-geometry-path"),
            commands: commands.clone(),
            style: resolve_style(style, Color::transparent(), Color::hex(INK)),
            marker_end: false,
        },
        GeometryNode::Text {
            id,
            x,
            y,
            text,
            font_size,
            anchor,
            color,
            weight,
        } => text_node(
            format!("{owner}/{id}"),
            Point { x: *x, y: *y },
            text.clone(),
            *font_size,
            *anchor,
            color.clone().unwrap_or_else(|| Color::hex(INK)),
            *weight,
            id,
            "typed geometry text lowered to native Scene2D text",
        ),
    }
}

fn geometry_origin(owner: &str, id: &str, pass: &str) -> Origin {
    Origin {
        hir_node: id.to_owned(),
        data_key: None,
        generated_by: pass.to_owned(),
        explanation: format!("geometry node {id} lowered losslessly inside {owner}"),
    }
}

fn geometry_group_bounds(children: &[GeometryNode]) -> Rect {
    let points = children
        .iter()
        .flat_map(geometry_points)
        .collect::<Vec<_>>();
    bounds_for_points(&points)
}

fn geometry_points(node: &GeometryNode) -> Vec<Point> {
    match node {
        GeometryNode::Group { children, .. } => children.iter().flat_map(geometry_points).collect(),
        GeometryNode::Rect {
            x,
            y,
            width,
            height,
            ..
        } => vec![
            Point { x: *x, y: *y },
            Point {
                x: x + width,
                y: y + height,
            },
        ],
        GeometryNode::Circle { cx, cy, radius, .. } => vec![
            Point {
                x: cx - radius,
                y: cy - radius,
            },
            Point {
                x: cx + radius,
                y: cy + radius,
            },
        ],
        GeometryNode::Line { from, to, .. } => vec![*from, *to],
        GeometryNode::Path { commands, .. } => path_points(commands),
        GeometryNode::Text {
            x, y, font_size, ..
        } => vec![
            Point {
                x: *x,
                y: y - font_size,
            },
            Point {
                x: *x + font_size * 8.0,
                y: *y,
            },
        ],
    }
}

fn resolve_style(style: &ShapeStyle, default_fill: Color, default_stroke: Color) -> ResolvedStyle {
    ResolvedStyle {
        fill: style.fill.clone().unwrap_or(default_fill),
        stroke: style.stroke.clone().unwrap_or(default_stroke),
        stroke_width: style.stroke_width,
        opacity: style.opacity,
    }
}

fn resolve_color(chart: &MirChart, scale_id: Option<&str>, category: Option<&str>) -> Color {
    let (Some(scale_id), Some(category)) = (scale_id, category) else {
        return Color::hex(BLUE);
    };
    if let Some(MirScale::OrdinalColor { domain, range, .. }) =
        chart.scales.iter().find(|scale| scale.id() == scale_id)
        && let Some(index) = domain.iter().position(|value| value == category)
    {
        return range[index].clone();
    }
    Color::hex(BLUE)
}

fn linear_scale(chart: &MirChart, id: &str) -> VizResult<([f64; 2], [f64; 2])> {
    chart
        .scales
        .iter()
        .find_map(|scale| match scale {
            MirScale::Linear {
                id: scale_id,
                domain,
                range,
                ..
            } if scale_id == id => Some((*domain, *range)),
            _ => None,
        })
        .ok_or_else(|| VizError::Diagnostic(format!("VIZ-SCENE-0002: missing linear scale {id:?}")))
}

fn band_scale<'a>(chart: &'a MirChart, id: &str) -> VizResult<(&'a [String], [f64; 2], f64)> {
    chart
        .scales
        .iter()
        .find_map(|scale| match scale {
            MirScale::Band {
                id: scale_id,
                domain,
                range,
                padding,
            } if scale_id == id => Some((domain.as_slice(), *range, *padding)),
            _ => None,
        })
        .ok_or_else(|| VizError::Diagnostic(format!("VIZ-SCENE-0003: missing band scale {id:?}")))
}

fn chart_plot_bounds(frame: vizir_core::Frame) -> [f64; 4] {
    [
        frame.x + 64.0,
        frame.y + 50.0,
        frame.x + frame.width - 30.0,
        frame.y + frame.height - 62.0,
    ]
}

fn frame_rect(frame: vizir_core::Frame) -> Rect {
    Rect {
        x: frame.x,
        y: frame.y,
        width: frame.width,
        height: frame.height,
    }
}

fn title_node(id: &str, title: &str, frame: vizir_core::Frame) -> SceneNode {
    text_node(
        format!("{id}/title"),
        Point {
            x: frame.x + 18.0,
            y: frame.y + 28.0,
        },
        title.to_owned(),
        18.0,
        TextAnchor::Start,
        Color::hex(INK),
        FontWeight::Bold,
        id,
        "view title retained from HIR",
    )
}

#[allow(clippy::too_many_arguments)]
fn text_node(
    id: String,
    position: Point,
    text: String,
    font_size: f64,
    anchor: TextAnchor,
    color: Color,
    weight: FontWeight,
    hir_node: &str,
    explanation: &str,
) -> SceneNode {
    let width = text.chars().count() as f64 * font_size * 0.58;
    let x = match anchor {
        TextAnchor::Start => position.x,
        TextAnchor::Middle => position.x - width / 2.0,
        TextAnchor::End => position.x - width,
    };
    SceneNode::Text {
        id,
        bounds: Rect {
            x,
            y: position.y - font_size,
            width,
            height: font_size * 1.25,
        },
        origin: Origin {
            hir_node: hir_node.to_owned(),
            data_key: None,
            generated_by: "shape-native-text".to_owned(),
            explanation: explanation.to_owned(),
        },
        position,
        text,
        font_size,
        anchor,
        color,
        weight,
    }
}

#[allow(clippy::too_many_arguments)]
fn line_node(
    id: String,
    from: Point,
    to: Point,
    stroke: Color,
    stroke_width: f64,
    opacity: f64,
    hir_node: &str,
    explanation: &str,
) -> SceneNode {
    SceneNode::Line {
        id,
        bounds: Rect::from_points(from, to),
        origin: Origin {
            hir_node: hir_node.to_owned(),
            data_key: None,
            generated_by: "build-guide-scene".to_owned(),
            explanation: explanation.to_owned(),
        },
        from,
        to,
        style: ResolvedStyle {
            fill: Color::transparent(),
            stroke,
            stroke_width,
            opacity,
        },
        marker_end: false,
    }
}

fn bounds_for_path(commands: &[PathCommand]) -> Rect {
    bounds_for_points(&path_points(commands))
}

fn path_points(commands: &[PathCommand]) -> Vec<Point> {
    commands
        .iter()
        .flat_map(|command| match command {
            PathCommand::Move { to } | PathCommand::Line { to } => vec![*to],
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => vec![*control1, *control2, *to],
            PathCommand::Close => Vec::new(),
        })
        .collect()
}

fn bounds_for_points(points: &[Point]) -> Rect {
    if points.is_empty() {
        return Rect::default();
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for point in points {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    Rect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    }
}

fn format_number(value: f64) -> String {
    if value.abs() >= 1000.0 {
        format!("{:.1}k", value / 1000.0)
    } else if value.fract().abs() < 0.001 {
        format!("{value:.0}")
    } else if value.abs() < 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    }
}
