use std::collections::BTreeMap;

use vizir_core::{
    ChartMark, Color, FontWeight, GuideKind, GuideOrient, MirChart, MirDiagram, MirGeometry,
    MirGeometryNode, MirGuide, MirScale, MirShapeStyle, MirView, Origin, PathCommand, Point, Rect,
    ResolvedStyle, Scene2D, SceneNode, ShapeStyle, TextAnchor, Transform2D, VizError, VizMir,
    VizResult, map_linear,
};

use crate::ResolvedThemeDefaults;
use crate::chart_layout::{ChartLayout, header_text_width, legend_domain};
use crate::layout::{LayeredLayoutProvider, LayoutProvider};
use crate::materialize::{MaterializationLimits, materialize_mir_marks};
use crate::text::TextSession;
use crate::tick_format::{NumericTickLabels, format_number};

const INK: &str = "#1C2736";
const MUTED: &str = "#596579";
const GRID: &str = "#D7DEE8";
const BLUE: &str = "#3B6EF5";
const SURFACE: &str = "#F7F9FC";

pub fn build_scene(mir: &VizMir) -> VizResult<Scene2D> {
    build_scene_with_limits(mir, MaterializationLimits::default())
}

pub fn build_scene_with_limits(mir: &VizMir, limits: MaterializationLimits) -> VizResult<Scene2D> {
    build_scene_with_defaults(mir, limits, None)
}

pub(crate) fn build_scene_with_defaults(
    mir: &VizMir,
    limits: MaterializationLimits,
    defaults: Option<&ResolvedThemeDefaults>,
) -> VizResult<Scene2D> {
    build_scene_with_context(mir, limits, defaults, None)
}
pub(crate) fn build_scene_with_context(
    mir: &VizMir,
    limits: MaterializationLimits,
    defaults: Option<&ResolvedThemeDefaults>,
    text: Option<&TextSession>,
) -> VizResult<Scene2D> {
    if let Some(text) = text {
        text.preflight_mir(mir)?;
    }
    let marks = materialize_mir_marks(mir, limits, true)?;
    let mut nodes = Vec::new();
    for (view, mark) in mir.views.iter().zip(&marks) {
        nodes.push(match view {
            MirView::Chart(chart) => build_chart(
                chart,
                mark.as_ref().expect("chart materialized"),
                defaults,
                text,
            )?,
            MirView::Diagram(diagram) => build_diagram(diagram, defaults, text)?,
            MirView::Geometry(geometry) => build_geometry(geometry, defaults)?,
        });
    }
    let scene = Scene2D {
        document_id: mir.document_id.clone(),
        width: mir.width,
        height: mir.height,
        background: mir.background.clone(),
        nodes,
        losses: mir.losses.clone(),
    };
    vizir_core::validate_scene(&scene).map_err(|diagnostics| VizError::validation(&diagnostics))?;
    match text {
        Some(text) => text.outline_scene(scene),
        None => Ok(scene),
    }
}

fn build_chart(
    chart: &MirChart,
    materialized: &ChartMark,
    defaults: Option<&ResolvedThemeDefaults>,
    text: Option<&TextSession>,
) -> VizResult<SceneNode> {
    let mut children = Vec::new();
    let guides = ChartGuides::resolve(chart)?;
    let ticks = NumericTickLabels::new_with_measurement(
        guides.bottom.and_then(|(guide, scale)| match scale {
            MirScale::Linear { domain, .. } => Some((*domain, guide.number_format.as_ref())),
            _ => None,
        }),
        guides.left.and_then(|(guide, scale)| match scale {
            MirScale::Linear { domain, .. } => Some((*domain, guide.number_format.as_ref())),
            _ => None,
        }),
        text.is_some(),
    )
    .map_err(VizError::Diagnostic)?;
    // Static 0.1 line/bar MIR may omit a legend guide. Preserve that legacy
    // implicit legend, but an explicit guide always owns its scale and layout.
    let legend_scale = guides.legend.map(|(_, scale)| scale).or_else(|| {
        let binding = match &chart.mark {
            ChartMark::Symbol { color, .. }
            | ChartMark::Line { color, .. }
            | ChartMark::Bar { color, .. } => color.as_ref(),
        }?;
        chart
            .scales
            .iter()
            .find(|scale| scale.id() == binding.scale)
    });
    let layout = ChartLayout::new_with_text(
        &chart.id,
        chart.frame,
        chart.title.as_deref(),
        guides.bottom.map(|(guide, _)| guide.label.as_str()),
        guides.left.map(|(guide, _)| guide.label.as_str()),
        legend_domain(legend_scale),
        text,
    )
    .map_err(VizError::Diagnostic)?
    .with_numeric_ticks_and_text(
        &chart.id,
        chart.frame,
        guides.bottom.map(|(guide, _)| guide.label.as_str()),
        ticks.as_ref(),
        text,
    )
    .map_err(VizError::Diagnostic)?;
    let categories = match guides.bottom {
        Some((_, MirScale::Band { domain, .. })) => domain.as_slice(),
        _ => &[],
    };
    let layout = layout
        .with_categories(&chart.id, categories, text)
        .map_err(VizError::Diagnostic)?;
    let plot = layout.plot;
    let (x_range, y_range) = match &chart.mark {
        ChartMark::Symbol { x, y, .. } | ChartMark::Line { x, y, .. } => (
            linear_scale(chart, &x.scale)?.1,
            linear_scale(chart, &y.scale)?.1,
        ),
        ChartMark::Bar {
            category, value, ..
        } => (
            band_scale(chart, &category.scale)?.1,
            linear_scale(chart, &value.scale)?.1,
        ),
    };
    if !ranges_match(
        x_range,
        [plot[0], plot[2]],
        chart.frame.x,
        chart.frame.width,
    ) || !ranges_match(
        y_range,
        [plot[3], plot[1]],
        chart.frame.y,
        chart.frame.height,
    ) {
        return Err(VizError::Diagnostic(format!(
            "VIZ-LAYOUT-0005: chart {:?} scale ranges do not match its header layout; normalize again from VizHIR before building the scene",
            chart.id
        )));
    }

    // Once checked, use the stored endpoints for guides as well as marks. A
    // serialized MIR may differ from recomputed layout by a rounding bit.
    let plot = [x_range[0], y_range[1], x_range[1], y_range[0]];
    guides.check_ranges(chart, plot)?;
    children.extend(build_grid_and_axes(
        chart,
        plot,
        &guides,
        ticks.as_ref(),
        defaults,
    ));
    match materialized {
        ChartMark::Symbol {
            id: mark_id,
            x,
            y,
            color,
            size,
            instances,
            ..
        } => {
            let x_scale = linear_scale(chart, &x.scale)?;
            let y_scale = linear_scale(chart, &y.scale)?;
            for item in instances {
                let center = Point {
                    x: map_linear(item.x, x_scale.0, x_scale.1),
                    y: map_linear(item.y, y_scale.0, y_scale.1),
                };
                let color = resolve_color(
                    chart,
                    color.as_ref().map(|binding| binding.scale.as_str()),
                    item.color_category.as_deref(),
                    defaults,
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
                        mir_node: mark_id.clone(),
                        data_key: Some(item.key.clone()),
                        data_lineage: vec![chart.source.clone()],
                        generated_by: "build-symbol-scene".to_owned(),
                        explanation: format!(
                            "datum {} mapped through scales {} and {}",
                            item.key, x.scale, y.scale
                        ),
                    },
                    center,
                    radius: *size,
                    style: ResolvedStyle {
                        fill: color,
                        stroke: defaults
                            .map(|d| d.point_interior.clone())
                            .unwrap_or_else(|| Color::hex("#FFFFFF")),
                        stroke_width: 1.5,
                        opacity: 0.9,
                    },
                });
            }
        }
        ChartMark::Line {
            id: mark_id,
            x,
            y,
            color,
            line_width,
            show_points,
            order_expression,
            series,
            ..
        } => {
            let x_scale = linear_scale(chart, &x.scale)?;
            let y_scale = linear_scale(chart, &y.scale)?;
            for series in series {
                let color = resolve_color(
                    chart,
                    color.as_ref().map(|binding| binding.scale.as_str()),
                    series.color_category.as_deref(),
                    defaults,
                );
                let points = series
                    .points
                    .iter()
                    .map(|item| Point {
                        x: map_linear(item.x, x_scale.0, x_scale.1),
                        y: map_linear(item.y, y_scale.0, y_scale.1),
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
                        mir_node: mark_id.clone(),
                        data_key: Some(series.key.clone()),
                        data_lineage: vec![chart.source.clone()],
                        generated_by: "build-line-scene".to_owned(),
                        explanation: format!(
                            "series {} sorted by {} and mapped through {} and {}",
                            series.key,
                            if order_expression == &x.expression {
                                "x"
                            } else {
                                order_expression.as_str()
                            },
                            x.scale,
                            y.scale
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
                                mir_node: mark_id.clone(),
                                data_key: Some(item.key.clone()),
                                data_lineage: vec![chart.source.clone()],
                                generated_by: "build-line-point-scene".to_owned(),
                                explanation: format!(
                                    "line datum {} retained for stable identity",
                                    item.key
                                ),
                            },
                            center: *point,
                            radius: 3.5,
                            style: ResolvedStyle {
                                fill: defaults
                                    .map(|d| d.point_interior.clone())
                                    .unwrap_or_else(|| Color::hex("#FFFFFF")),
                                stroke: color.clone(),
                                stroke_width: 2.0,
                                opacity: 1.0,
                            },
                        });
                    }
                }
            }
        }
        ChartMark::Bar {
            id: mark_id,
            category,
            value,
            color,
            instances,
            ..
        } => {
            let (categories, range, padding) = band_scale(chart, &category.scale)?;
            let value_scale = linear_scale(chart, &value.scale)?;
            let baseline = map_linear(0.0, value_scale.0, value_scale.1);
            let step = (range[1] - range[0]) / categories.len().max(1) as f64;
            let width = step * (1.0 - padding);
            for item in instances {
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
                let y = map_linear(item.value, value_scale.0, value_scale.1);
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
                        mir_node: mark_id.clone(),
                        data_key: Some(item.key.clone()),
                        data_lineage: vec![chart.source.clone()],
                        generated_by: "build-bar-scene".to_owned(),
                        explanation: format!(
                            "bar {} mapped through {} and zero-preserving {}",
                            item.key, category.scale, value.scale
                        ),
                    },
                    radius: 5.0,
                    style: ResolvedStyle {
                        fill: resolve_color(
                            chart,
                            color.as_ref().map(|binding| binding.scale.as_str()),
                            item.color_category.as_deref(),
                            defaults,
                        ),
                        stroke: Color::transparent(),
                        stroke_width: 0.0,
                        opacity: 0.92,
                    },
                });
            }
        }
    }

    children.extend(build_legend(
        chart,
        guides.legend.map(|(guide, _)| guide),
        legend_scale,
        &layout,
        defaults,
    ));
    if let Some(title) = &chart.title {
        children.push(header_text_envelope(title_node(
            &chart.id,
            title,
            chart.frame,
            defaults,
        )));
    }

    if let Some(text) = text {
        text.check_chart(&children, chart.frame, plot)?;
    }
    Ok(SceneNode::Group {
        id: chart.id.clone(),
        bounds: frame_rect(chart.frame),
        origin: Origin {
            hir_node: chart.id.clone(),
            mir_node: chart.id.clone(),
            data_key: None,
            data_lineage: vec![chart.source.clone()],
            generated_by: "build-chart-scene".to_owned(),
            explanation: chart.provenance.join("; "),
        },
        transform: Transform2D::default(),
        opacity: 1.0,
        children,
    })
}

// JSON parsing and layout recomputation can round independently. Permit only
// a few floating-point rounding units at the axis-coordinate scale, including
// its operands so translations near zero do not amplify cancellation error.
// This is not a visual/layout tolerance (about 7e-13 for a 760px frame).
fn ranges_match(actual: [f64; 2], expected: [f64; 2], origin: f64, extent: f64) -> bool {
    actual.into_iter().zip(expected).all(|(actual, expected)| {
        let magnitude = actual
            .abs()
            .max(expected.abs())
            .max(origin.abs())
            .max(extent.abs())
            .max(1.0);
        actual.is_finite()
            && expected.is_finite()
            && (actual - expected).abs() <= 4.0 * f64::EPSILON * magnitude
    })
}

// Guide IDs and scale IDs are opaque references, not naming conventions.
#[derive(Default)]
struct ChartGuides<'a> {
    bottom: Option<(&'a MirGuide, &'a MirScale)>,
    left: Option<(&'a MirGuide, &'a MirScale)>,
    legend: Option<(&'a MirGuide, &'a MirScale)>,
}

impl<'a> ChartGuides<'a> {
    fn resolve(chart: &'a MirChart) -> VizResult<Self> {
        let mut guides = Self::default();
        for guide in &chart.guides {
            let scale = chart
                .scales
                .iter()
                .find(|scale| scale.id() == guide.scale)
                .ok_or_else(|| {
                    VizError::Diagnostic(format!(
                        "VIZ-RESOLVE-0006: guide {:?} references unknown scale {:?}",
                        guide.id, guide.scale
                    ))
                })?;
            let compatible = matches!(
                (&guide.kind, scale),
                (
                    GuideKind::Axis,
                    MirScale::Linear { .. } | MirScale::Band { .. }
                ) | (GuideKind::Legend, MirScale::OrdinalColor { .. })
            );
            if !compatible {
                return Err(VizError::Diagnostic(format!(
                    "VIZ-TYPE-0203: guide {:?} has an incompatible scale {:?}",
                    guide.id, guide.scale
                )));
            }
            let slot = match (&guide.kind, &guide.orient, scale) {
                (GuideKind::Axis, GuideOrient::Bottom, _) => &mut guides.bottom,
                (GuideKind::Axis, GuideOrient::Left, MirScale::Linear { .. }) => &mut guides.left,
                (GuideKind::Legend, GuideOrient::Right, _) => &mut guides.legend,
                _ => {
                    return Err(VizError::Diagnostic(format!(
                        "VIZ-SCENE-0004: guide {:?} has an unsupported kind, orientation, or scale combination",
                        guide.id
                    )));
                }
            };
            if slot.is_some() {
                return Err(VizError::Diagnostic(format!(
                    "VIZ-SCENE-0004: guide {:?} duplicates a guide slot; only one bottom axis, left axis, and right legend are supported",
                    guide.id
                )));
            }
            *slot = Some((guide, scale));
        }
        Ok(guides)
    }

    fn check_ranges(&self, chart: &MirChart, plot: [f64; 4]) -> VizResult<()> {
        for (resolved, expected, origin, extent) in [
            (
                self.bottom,
                [plot[0], plot[2]],
                chart.frame.x,
                chart.frame.width,
            ),
            (
                self.left,
                [plot[3], plot[1]],
                chart.frame.y,
                chart.frame.height,
            ),
        ] {
            if let Some((guide, MirScale::Linear { range, .. } | MirScale::Band { range, .. })) =
                resolved
                && !ranges_match(*range, expected, origin, extent)
            {
                return Err(VizError::Diagnostic(format!(
                    "VIZ-LAYOUT-0005: guide {:?} scale {:?} range does not match its chart plot; normalize again from VizHIR before building the scene",
                    guide.id, guide.scale
                )));
            }
        }
        Ok(())
    }
}

fn build_grid_and_axes(
    chart: &MirChart,
    plot: [f64; 4],
    guides: &ChartGuides<'_>,
    ticks: Option<&NumericTickLabels>,
    defaults: Option<&ResolvedThemeDefaults>,
) -> Vec<SceneNode> {
    let mut nodes = Vec::new();
    let x_scale = guides.bottom.map(|(_, scale)| scale);
    let y_scale = guides.left.map(|(_, scale)| scale);

    if y_scale.is_some() {
        for index in 0..=5 {
            let fraction = index as f64 / 5.0;
            let y = plot[1] + (plot[3] - plot[1]) * fraction;
            nodes.push(line_node(
                format!("{}/grid/y/{index}", chart.id),
                Point { x: plot[0], y },
                Point { x: plot[2], y },
                defaults
                    .map(|d| d.grid.clone())
                    .unwrap_or_else(|| Color::hex(GRID)),
                1.0,
                0.7,
                &chart.id,
                "axis guide generated from normalized scale",
            ));
            if let Some(MirScale::Linear { domain, .. }) = y_scale {
                let value = domain[1] + (domain[0] - domain[1]) * fraction;
                nodes.push(numeric_tick_envelope(
                    text_node(
                        format!("{}/axis/y/label/{index}", chart.id),
                        Point {
                            x: plot[0] - 10.0,
                            y: y + 4.0,
                        },
                        ticks
                            .map(|ticks| ticks.y[index].clone())
                            .unwrap_or_else(|| format_number(value, None)),
                        11.0,
                        TextAnchor::End,
                        defaults
                            .map(|d| d.muted.clone())
                            .unwrap_or_else(|| Color::hex(MUTED)),
                        FontWeight::Regular,
                        &chart.id,
                        "tick label generated from linear scale domain",
                    ),
                    ticks,
                ));
            }
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
                defaults
                    .map(|d| d.muted.clone())
                    .unwrap_or_else(|| Color::hex(MUTED)),
                FontWeight::Regular,
                &chart.id,
                "category label generated from band scale domain",
            ));
        }
    }
    if x_scale.is_some() {
        for index in 0..=5 {
            let fraction = index as f64 / 5.0;
            let x = plot[0] + (plot[2] - plot[0]) * fraction;
            nodes.push(line_node(
                format!("{}/grid/x/{index}", chart.id),
                Point { x, y: plot[1] },
                Point { x, y: plot[3] },
                defaults
                    .map(|d| d.grid.clone())
                    .unwrap_or_else(|| Color::hex(GRID)),
                1.0,
                0.45,
                &chart.id,
                "axis guide generated from normalized scale",
            ));
            if let Some(MirScale::Linear { domain, .. }) = x_scale {
                let value = domain[0] + (domain[1] - domain[0]) * fraction;
                nodes.push(numeric_tick_envelope(
                    text_node(
                        format!("{}/axis/x/label/{index}", chart.id),
                        Point {
                            x,
                            y: plot[3] + 20.0,
                        },
                        ticks
                            .map(|ticks| ticks.x[index].clone())
                            .unwrap_or_else(|| format_number(value, None)),
                        11.0,
                        TextAnchor::Middle,
                        defaults
                            .map(|d| d.muted.clone())
                            .unwrap_or_else(|| Color::hex(MUTED)),
                        FontWeight::Regular,
                        &chart.id,
                        "tick label generated from linear scale domain",
                    ),
                    ticks,
                ));
            }
        }
    }

    if x_scale.is_some() {
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
            defaults
                .map(|d| d.muted.clone())
                .unwrap_or_else(|| Color::hex(MUTED)),
            1.4,
            1.0,
            &chart.id,
            "bottom axis emitted from explicit MIR guide",
        ));
    }
    if y_scale.is_some() {
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
            defaults
                .map(|d| d.muted.clone())
                .unwrap_or_else(|| Color::hex(MUTED)),
            1.4,
            1.0,
            &chart.id,
            "left axis emitted from explicit MIR guide",
        ));
    }

    for guide in &chart.guides {
        if guide.kind != vizir_core::GuideKind::Axis {
            continue;
        }
        match guide.orient {
            vizir_core::GuideOrient::Bottom => nodes.push(header_text_envelope(text_node(
                format!("{}/axis/x/title", chart.id),
                Point {
                    x: (plot[0] + plot[2]) / 2.0,
                    y: chart.frame.y + chart.frame.height - 16.0,
                },
                guide.label.clone(),
                12.5,
                TextAnchor::Middle,
                defaults
                    .map(|d| d.ink.clone())
                    .unwrap_or_else(|| Color::hex(INK)),
                FontWeight::Medium,
                &chart.id,
                "axis title emitted from explicit MIR guide",
            ))),
            vizir_core::GuideOrient::Left => nodes.push(header_text_envelope(text_node(
                format!("{}/axis/y/title", chart.id),
                Point {
                    x: chart.frame.x + 16.0,
                    y: plot[1] - 12.0,
                },
                guide.label.clone(),
                12.5,
                TextAnchor::Start,
                defaults
                    .map(|d| d.ink.clone())
                    .unwrap_or_else(|| Color::hex(INK)),
                FontWeight::Medium,
                &chart.id,
                "axis title emitted from explicit MIR guide",
            ))),
            vizir_core::GuideOrient::Right => {}
        }
    }

    nodes
}

fn build_legend(
    chart: &MirChart,
    guide: Option<&MirGuide>,
    scale: Option<&MirScale>,
    layout: &ChartLayout,
    defaults: Option<&ResolvedThemeDefaults>,
) -> Vec<SceneNode> {
    let Some(MirScale::OrdinalColor { domain, range, .. }) = scale else {
        return Vec::new();
    };
    let mut nodes = Vec::new();
    for (index, (label, color)) in domain.iter().zip(range).enumerate() {
        let Point { x, y } = layout.legend[index];
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
                mir_node: guide
                    .map(|guide| guide.id.clone())
                    .unwrap_or_else(|| format!("{}/guides/color-legend", chart.id)),
                data_key: None,
                data_lineage: vec![chart.source.clone()],
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
        nodes.push(header_text_envelope(text_node(
            format!("{}/legend/{index}/label", chart.id),
            Point {
                x: x + 13.0,
                y: y + 4.0,
            },
            label.clone(),
            10.5,
            TextAnchor::Start,
            defaults
                .map(|d| d.muted.clone())
                .unwrap_or_else(|| Color::hex(MUTED)),
            FontWeight::Regular,
            &chart.id,
            "legend label generated from ordinal color scale",
        )));
    }
    nodes
}

fn build_diagram(
    diagram: &MirDiagram,
    defaults: Option<&ResolvedThemeDefaults>,
    text: Option<&TextSession>,
) -> VizResult<SceneNode> {
    if let Some(text) = text {
        for node in &diagram.nodes {
            if text.width(
                &node.label,
                if node.label.chars().count() > 22 {
                    10.5
                } else {
                    13.0
                },
                FontWeight::Medium,
            )? > 134.0
            {
                return Err(VizError::Diagnostic(format!(
                    "VIZ-TEXT-0006: diagram node {:?} label exceeds its 134px text region; shorten the original label",
                    node.id
                )));
            }
        }
    }
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
        let edge_style = resolve_style(
            &edge.style,
            Color::transparent(),
            defaults
                .map(|d| d.edge.clone())
                .unwrap_or_else(|| Color::hex("#8793A5")),
        );
        children.push(SceneNode::Path {
            id: format!("{}/edge/{edge_index}-{}-{}", diagram.id, edge.from, edge.to),
            bounds: Rect::from_points(start, end),
            origin: Origin {
                hir_node: diagram.id.clone(),
                mir_node: format!("{}/edges/{edge_index}", diagram.id),
                data_key: Some(format!("{}->{}", edge.from, edge.to)),
                data_lineage: Vec::new(),
                generated_by: "route-diagram-edge".to_owned(),
                explanation: format!(
                    "edge {} -> {} routed after {}",
                    edge.from, edge.to, layout.explanation
                ),
            },
            commands,
            style: edge_style.clone(),
            marker_end: defaults.is_none(),
        });
        if defaults.is_some() {
            children.push(arrow_node(diagram, edge_index, end, control2, &edge_style)?);
        }
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
                defaults
                    .map(|d| d.muted.clone())
                    .unwrap_or_else(|| Color::hex(MUTED)),
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
            group_colors.entry(group.clone()).or_insert_with(|| {
                defaults
                    .map(|d| d.group_fills[next % d.group_fills.len()].clone())
                    .unwrap_or_else(|| Color::hex(group_palette[next % group_palette.len()]))
            });
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
            .unwrap_or_else(|| {
                defaults
                    .map(|d| d.node_fill.clone())
                    .unwrap_or_else(|| Color::hex(SURFACE))
            });
        children.push(SceneNode::Rect {
            id: format!("{}/node/{}/shape", diagram.id, node.id),
            bounds,
            origin: Origin {
                hir_node: node.id.clone(),
                mir_node: format!("{}/nodes/{}", diagram.id, node.id),
                data_key: Some(node.id.clone()),
                data_lineage: Vec::new(),
                generated_by: "build-diagram-node".to_owned(),
                explanation: format!("node placed by {}; stable id preserved", layout.explanation),
            },
            radius: 14.0,
            style: resolve_style(
                &node.style,
                default_fill,
                defaults
                    .map(|d| d.node_stroke.clone())
                    .unwrap_or_else(|| Color::hex("#B8C3D3")),
            ),
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
            defaults
                .map(|d| d.ink.clone())
                .unwrap_or_else(|| Color::hex(INK)),
            FontWeight::Medium,
            &node.id,
            "diagram label positioned inside resolved node bounds",
        ));
        if let Some(text) = text {
            text.check_text_box(
                children.last().expect("label just added"),
                Rect {
                    x: bounds.x + 8.0,
                    y: bounds.y + 4.0,
                    width: bounds.width - 16.0,
                    height: bounds.height - 8.0,
                },
            )?;
        }
    }
    if let Some(title) = &diagram.title {
        children.push(title_node(&diagram.id, title, diagram.frame, defaults));
    }
    Ok(SceneNode::Group {
        id: diagram.id.clone(),
        bounds: frame_rect(diagram.frame),
        origin: Origin {
            hir_node: diagram.id.clone(),
            mir_node: diagram.id.clone(),
            data_key: None,
            data_lineage: Vec::new(),
            generated_by: "build-diagram-scene".to_owned(),
            explanation: format!("{}; {}", diagram.provenance.join("; "), layout.explanation),
        },
        transform: Transform2D::default(),
        opacity: 1.0,
        children,
    })
}

fn build_geometry(
    geometry: &MirGeometry,
    defaults: Option<&ResolvedThemeDefaults>,
) -> VizResult<SceneNode> {
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
            defaults
                .map(|d| d.ink.clone())
                .unwrap_or_else(|| Color::hex(INK)),
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
            mir_node: geometry.id.clone(),
            data_key: None,
            data_lineage: Vec::new(),
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

fn lower_geometry_node(node: &MirGeometryNode, owner: &str) -> SceneNode {
    match node {
        MirGeometryNode::Group {
            id,
            transform,
            opacity,
            children,
        } => SceneNode::Group {
            id: format!("{owner}/{id}"),
            bounds: geometry_group_bounds(children),
            origin: geometry_origin(owner, id, "lower-geometry-group"),
            transform: *transform,
            opacity: *opacity,
            children: children
                .iter()
                .map(|child| lower_geometry_node(child, owner))
                .collect(),
        },
        MirGeometryNode::Rect {
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
            style: resolve_mir_style(style),
        },
        MirGeometryNode::Circle {
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
            style: resolve_mir_style(style),
        },
        MirGeometryNode::Line {
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
            style: resolve_mir_style(style),
            marker_end: false,
        },
        MirGeometryNode::Path {
            id,
            commands,
            style,
        } => SceneNode::Path {
            id: format!("{owner}/{id}"),
            bounds: bounds_for_path(commands),
            origin: geometry_origin(owner, id, "lower-geometry-path"),
            commands: commands.clone(),
            style: resolve_mir_style(style),
            marker_end: false,
        },
        MirGeometryNode::Text {
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
            color.clone(),
            *weight,
            id,
            "typed geometry text lowered to native Scene2D text",
        ),
    }
}

fn geometry_origin(owner: &str, id: &str, pass: &str) -> Origin {
    Origin {
        hir_node: id.to_owned(),
        mir_node: format!("{owner}/{id}"),
        data_key: None,
        data_lineage: Vec::new(),
        generated_by: pass.to_owned(),
        explanation: format!("geometry node {id} lowered losslessly inside {owner}"),
    }
}

fn geometry_group_bounds(children: &[MirGeometryNode]) -> Rect {
    let points = children
        .iter()
        .flat_map(geometry_points)
        .collect::<Vec<_>>();
    bounds_for_points(&points)
}

fn geometry_points(node: &MirGeometryNode) -> Vec<Point> {
    match node {
        MirGeometryNode::Group { children, .. } => {
            children.iter().flat_map(geometry_points).collect()
        }
        MirGeometryNode::Rect {
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
        MirGeometryNode::Circle { cx, cy, radius, .. } => vec![
            Point {
                x: cx - radius,
                y: cy - radius,
            },
            Point {
                x: cx + radius,
                y: cy + radius,
            },
        ],
        MirGeometryNode::Line { from, to, .. } => vec![*from, *to],
        MirGeometryNode::Path { commands, .. } => path_points(commands),
        MirGeometryNode::Text {
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

fn resolve_mir_style(style: &MirShapeStyle) -> ResolvedStyle {
    ResolvedStyle {
        fill: style.fill.clone(),
        stroke: style.stroke.clone(),
        stroke_width: style.stroke_width,
        opacity: style.opacity,
    }
}

fn resolve_color(
    chart: &MirChart,
    scale_id: Option<&str>,
    category: Option<&str>,
    defaults: Option<&ResolvedThemeDefaults>,
) -> Color {
    let (Some(scale_id), Some(category)) = (scale_id, category) else {
        return defaults
            .map(|d| d.mark.clone())
            .unwrap_or_else(|| Color::hex(BLUE));
    };
    if let Some(MirScale::OrdinalColor { domain, range, .. }) =
        chart.scales.iter().find(|scale| scale.id() == scale_id)
        && let Some(index) = domain.iter().position(|value| value == category)
    {
        return range[index].clone();
    }
    defaults
        .map(|d| d.mark.clone())
        .unwrap_or_else(|| Color::hex(BLUE))
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
                ..
            } if scale_id == id => Some((domain.as_slice(), *range, *padding)),
            _ => None,
        })
        .ok_or_else(|| VizError::Diagnostic(format!("VIZ-SCENE-0003: missing band scale {id:?}")))
}

// Header allocation and scene metadata use the same conservative envelopes.
fn header_text_envelope(mut node: SceneNode) -> SceneNode {
    if let SceneNode::Text {
        bounds,
        text,
        font_size,
        position,
        anchor,
        ..
    } = &mut node
    {
        bounds.width = header_text_width(text, *font_size);
        bounds.x = match anchor {
            TextAnchor::Start => position.x,
            TextAnchor::Middle => position.x - bounds.width / 2.0,
            TextAnchor::End => position.x - bounds.width,
        };
    }
    node
}

fn frame_rect(frame: vizir_core::Frame) -> Rect {
    Rect {
        x: frame.x,
        y: frame.y,
        width: frame.width,
        height: frame.height,
    }
}

fn title_node(
    id: &str,
    title: &str,
    frame: vizir_core::Frame,
    defaults: Option<&ResolvedThemeDefaults>,
) -> SceneNode {
    text_node(
        format!("{id}/title"),
        Point {
            x: frame.x + 18.0,
            y: frame.y + 28.0,
        },
        title.to_owned(),
        18.0,
        TextAnchor::Start,
        defaults
            .map(|d| d.ink.clone())
            .unwrap_or_else(|| Color::hex(INK)),
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
            mir_node: hir_node.to_owned(),
            data_key: None,
            data_lineage: Vec::new(),
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
            mir_node: hir_node.to_owned(),
            data_key: None,
            data_lineage: Vec::new(),
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

fn numeric_tick_envelope(node: SceneNode, ticks: Option<&NumericTickLabels>) -> SceneNode {
    if ticks.is_some() {
        header_text_envelope(node)
    } else {
        node
    }
}

// The legacy SVG marker uses markerUnits=strokeWidth, a 7x7 viewport and
// refX=9 on a 10-unit triangle. Resolve that same triangle once in Scene2D.
fn arrow_node(
    diagram: &MirDiagram,
    edge_index: usize,
    end: Point,
    control: Point,
    edge_style: &ResolvedStyle,
) -> VizResult<SceneNode> {
    let edge = &diagram.edges[edge_index];
    let dx = end.x - control.x;
    let dy = end.y - control.y;
    let length = dx.hypot(dy);
    if !length.is_finite() || length == 0.0 || !edge_style.stroke_width.is_finite() {
        return Err(crate::theme::theme_error(
            "0005",
            "cannot resolve a finite diagram arrow tangent",
        ));
    }
    let unit = edge_style.stroke_width * 0.7;
    let direction = Point {
        x: dx / length,
        y: dy / length,
    };
    let project = |x: f64, y: f64| Point {
        x: end.x + unit * (direction.x * x - direction.y * y),
        y: end.y + unit * (direction.y * x + direction.x * y),
    };
    let vertices = [project(-9.0, -5.0), project(1.0, 0.0), project(-9.0, 5.0)];
    if vertices
        .iter()
        .any(|p| !p.x.is_finite() || !p.y.is_finite())
    {
        return Err(crate::theme::theme_error(
            "0005",
            "diagram arrow coordinates overflow",
        ));
    }
    let min = Point {
        x: vertices.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
        y: vertices.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
    };
    let max = Point {
        x: vertices
            .iter()
            .map(|p| p.x)
            .fold(f64::NEG_INFINITY, f64::max),
        y: vertices
            .iter()
            .map(|p| p.y)
            .fold(f64::NEG_INFINITY, f64::max),
    };
    Ok(SceneNode::Path {
        id: format!("{}/edge/{edge_index}-{}-{}/arrow", diagram.id, edge.from, edge.to),
        bounds: Rect::from_points(min, max),
        origin: Origin { hir_node: diagram.id.clone(), mir_node: format!("{}/edges/{edge_index}", diagram.id), data_key: Some(format!("{}->{}", edge.from, edge.to)), data_lineage: Vec::new(), generated_by: "resolve-diagram-arrow".to_owned(), explanation: "arrow triangle resolved from edge tangent and authored stroke before target emission".to_owned() },
        commands: vec![PathCommand::Move { to: vertices[0] }, PathCommand::Line { to: vertices[1] }, PathCommand::Line { to: vertices[2] }, PathCommand::Close],
        style: ResolvedStyle { fill: edge_style.stroke.clone(), stroke: Color::transparent(), stroke_width: 0.0, opacity: edge_style.opacity },
        marker_end: false,
    })
}
