use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use vizir_core::{
    AreaChart, BarChart, ChartMark, Color, ColorEncoding, CoordinateSpace2D, CoordinateSpaceKind,
    Document, Expression, GeometryNode, GuideKind, GuideOrient, HeatmapChart, LineChart, MirChart,
    MirDataNode, MirDataOperator, MirDataSchema, MirDiagram, MirGeometry, MirGeometryNode,
    MirGuide, MirScale, MirShapeStyle, MirView, ScaleBinding, ScatterChart, SpatialUnit,
    Transform2D, TypeEnvironment, TypedExpression, UpdateMode, ValueType, View, VizError, VizMir,
    VizResult, type_expression,
};

use crate::ResolvedThemeDefaults;
use crate::chart_layout::ChartLayout;
use crate::materialize::{
    Budget, MaterializationLimits, materialize_mark, preflight_document, preflight_hir_bindings,
};
use crate::text::TextSession;
use crate::tick_format::NumericTickLabels;

const DEFAULT_PALETTE: [&str; 8] = [
    "#3B6EF5", "#EB5E55", "#18A999", "#F2A541", "#7A5AF8", "#D94891", "#4B8B3B", "#65758B",
];

const DOCUMENT_SPACE: &str = "space/document";

pub fn lower_to_mir(document: &Document) -> VizResult<VizMir> {
    lower_to_mir_with_defaults(document, None, MaterializationLimits::default())
}

pub(crate) fn lower_to_mir_with_defaults(
    document: &Document,
    defaults: Option<&ResolvedThemeDefaults>,
    limits: MaterializationLimits,
) -> VizResult<VizMir> {
    lower_to_mir_with_context(document, defaults, limits, None)
}

pub(crate) fn lower_to_mir_with_context(
    document: &Document,
    defaults: Option<&ResolvedThemeDefaults>,
    limits: MaterializationLimits,
    text: Option<&TextSession>,
) -> VizResult<VizMir> {
    let mut budget = Budget::new(limits);
    preflight_document(document, &mut budget)?;
    if let Some(text) = text {
        text.preflight_document(document)?;
    }
    vizir_core::validate_document(document)
        .map_err(|diagnostics| VizError::validation(&diagnostics))?;
    let data = lower_data(document).map_err(lowering_error)?;
    let mut expressions = BTreeMap::new();
    let mut spaces = BTreeMap::from([(
        DOCUMENT_SPACE.to_owned(),
        CoordinateSpace2D {
            id: DOCUMENT_SPACE.to_owned(),
            kind: CoordinateSpaceKind::Document,
            parent: None,
            unit: SpatialUnit::SceneUnit,
            transform_to_parent: Transform2D::default(),
        },
    )]);
    let mut views = Vec::with_capacity(document.views.len());
    for view in &document.views {
        views.push(match view {
            View::Scatter(chart) => MirView::Chart(Box::new(
                lower_scatter(
                    document,
                    chart,
                    &data,
                    &mut expressions,
                    &mut budget,
                    defaults,
                    text,
                )
                .map_err(lowering_error)?,
            )),
            View::Line(chart) => MirView::Chart(Box::new(
                lower_line(
                    document,
                    chart,
                    &data,
                    &mut expressions,
                    &mut budget,
                    defaults,
                    text,
                )
                .map_err(lowering_error)?,
            )),
            View::Area(chart) => MirView::Chart(Box::new(
                lower_area(
                    document,
                    chart,
                    &data,
                    &mut expressions,
                    &mut budget,
                    defaults,
                    text,
                )
                .map_err(lowering_error)?,
            )),
            View::Heatmap(chart) => MirView::Chart(Box::new(
                lower_heatmap(
                    document,
                    chart,
                    &data,
                    &mut expressions,
                    &mut budget,
                    defaults,
                    text,
                )
                .map_err(lowering_error)?,
            )),
            View::Bar(chart) => MirView::Chart(Box::new(
                lower_bar(
                    document,
                    chart,
                    &data,
                    &mut expressions,
                    &mut budget,
                    defaults,
                    text,
                )
                .map_err(lowering_error)?,
            )),
            View::Diagram(diagram) => MirView::Diagram(MirDiagram {
                id: diagram.id.clone(),
                title: diagram.title.clone(),
                frame: diagram.frame,
                space: DOCUMENT_SPACE.to_owned(),
                nodes: diagram.nodes.clone(),
                edges: diagram.edges.clone(),
                layout_request: vizir_core::LayoutRequest {
                    id: format!("{}/layout", diagram.id),
                    algorithm: diagram.layout.clone(),
                    node_ids: diagram.nodes.iter().map(|node| node.id.clone()).collect(),
                    seed: 42,
                },
                provenance: vec![
                    "diagram.graph lowered to topology plus an explicit layout request".to_owned(),
                    format!("layout algorithm resolved to {:?}", diagram.layout),
                ],
            }),
            View::Geometry(geometry) => {
                let space = format!("space/{}/local", geometry.id);
                spaces.insert(
                    space.clone(),
                    CoordinateSpace2D {
                        id: space.clone(),
                        kind: CoordinateSpaceKind::ViewLocal,
                        parent: Some(DOCUMENT_SPACE.to_owned()),
                        unit: SpatialUnit::SceneUnit,
                        transform_to_parent: Transform2D {
                            translate: vizir_core::Point {
                                x: geometry.frame.x,
                                y: geometry.frame.y,
                            },
                            ..Transform2D::default()
                        },
                    },
                );
                MirView::Geometry(MirGeometry {
                    id: geometry.id.clone(),
                    title: geometry.title.clone(),
                    frame: geometry.frame,
                    space,
                    children: geometry
                        .children
                        .iter()
                        .map(|node| lower_geometry_node(node, defaults))
                        .collect(),
                    provenance: vec![
                        "geometry.scene defaults expanded into normalized portable primitives"
                            .to_owned(),
                        "view-local coordinates linked explicitly to the document space".to_owned(),
                    ],
                })
            }
        });
    }

    // Membership is resolved before layout; only the validated owner suppresses
    // local guides. Keep every categorical scale and mark binding intact.
    for view in &mut views {
        if let MirView::Chart(chart) = view
            && crate::shared_legend::hir_owns(document, &chart.id)
        {
            chart.guides.retain(|guide| guide.kind != GuideKind::Legend);
        }
    }
    let shared_legend = crate::shared_legend::lower_owner(document, &views)?;
    let plot_alignment = crate::plot_alignment::lower_group(document, &views)?;
    let mut mir = VizMir {
        version: document.version.clone(),
        source_hir_version: document.version.clone(),
        document_id: document.id.clone(),
        width: document.width,
        height: document.height,
        background: document.background.clone(),
        spaces,
        data,
        expressions,
        views,
        losses: Vec::new(),
        shared_legend,
        plot_alignment,
    };
    if mir.shared_legend.is_some() {
        vizir_core::validate_mir(&mir).map_err(|d| VizError::validation(&d))?;
        crate::shared_legend::build(&mir, defaults, text)?;
    }
    crate::plot_alignment::preflight_mir(&mir, &mut budget)?;
    crate::plot_alignment::finalize(&mut mir, text)?;
    crate::heatmap::check_mir_output(&mir)?;
    Ok(mir)
}

fn lower_scatter(
    document: &Document,
    chart: &ScatterChart,
    data: &BTreeMap<String, MirDataNode>,
    expressions: &mut BTreeMap<String, TypedExpression>,
    budget: &mut Budget,
    defaults: Option<&ResolvedThemeDefaults>,
    text: Option<&TextSession>,
) -> Result<MirChart, String> {
    let dataset = dataset(document, &chart.dataset)?;
    let source = data_id(&chart.dataset);
    preflight_hir_bindings(&data[&source], budget).map_err(|e| e.to_string())?;
    let row_variable = row_variable(&chart.id);
    let key_expression = register_field_expression(
        expressions,
        data,
        &source,
        &row_variable,
        &chart.id,
        "key",
        &dataset.key,
    )?;
    let x_expression = register_field_expression(
        expressions,
        data,
        &source,
        &row_variable,
        &chart.id,
        "x",
        &chart.x.field,
    )?;
    let y_expression = register_field_expression(
        expressions,
        data,
        &source,
        &row_variable,
        &chart.id,
        "y",
        &chart.y.field,
    )?;
    let color_expression = chart
        .color
        .as_ref()
        .map(|encoding| {
            register_field_expression(
                expressions,
                data,
                &source,
                &row_variable,
                &chart.id,
                "color",
                &encoding.field,
            )
        })
        .transpose()?;
    let plan = ChartMark::Symbol {
        id: format!("{}/marks/points", chart.id),
        x: scale_binding(format!("{}/x", chart.id), x_expression),
        y: scale_binding(format!("{}/y", chart.id), y_expression),
        color: color_expression
            .map(|expression| scale_binding(format!("{}/color", chart.id), expression)),
        size: chart.point_size,
        instances: Vec::new(),
    };
    let mark = materialize_mark(
        &data[&source],
        &row_variable,
        &key_expression,
        &plan,
        expressions,
        &chart.id,
        budget,
    )
    .map_err(|e| e.to_string())?;
    let ChartMark::Symbol { instances, .. } = &mark else {
        unreachable!()
    };
    let x_values = instances.iter().map(|p| p.x).collect::<Vec<_>>();
    let y_values = instances.iter().map(|p| p.y).collect::<Vec<_>>();
    let x_domain = chart
        .x
        .domain
        .unwrap_or_else(|| nice_domain(extent(&x_values), false));
    let y_domain = chart
        .y
        .domain
        .unwrap_or_else(|| nice_domain(extent(&y_values), false));
    let color_scale = materialized_color_scale(
        &chart.id,
        chart.color.as_ref(),
        instances.iter().filter_map(|p| p.color_category.as_ref()),
        defaults,
    );
    let ticks = NumericTickLabels::new_with_measurement(
        Some((x_domain, chart.x.number_format())),
        Some((y_domain, chart.y.number_format())),
        text.is_some(),
        [chart.x.domain.is_some(), chart.y.domain.is_some()],
    )?;
    let plot = ChartLayout::new_with_text(
        &chart.id,
        chart.frame,
        chart.title.as_deref(),
        Some(chart.x.label.as_deref().unwrap_or(&chart.x.field)),
        Some(chart.y.label.as_deref().unwrap_or(&chart.y.field)),
        crate::shared_legend::local_domain(document, &chart.id, color_scale.as_ref()),
        text,
    )?
    .with_numeric_ticks_and_text(
        &chart.id,
        chart.frame,
        Some(chart.x.label.as_deref().unwrap_or(&chart.x.field)),
        ticks.as_ref(),
        text,
    )?
    .plot;
    let mut scales = vec![
        MirScale::Linear {
            id: format!("{}/x", chart.id),
            domain: x_domain,
            out_of_domain: chart
                .x
                .domain
                .map(|_| vizir_core::NumericOutOfDomain::Reject),
            range: [plot[0], plot[2]],
            range_space: DOCUMENT_SPACE.to_owned(),
            zero: false,
        },
        MirScale::Linear {
            id: format!("{}/y", chart.id),
            domain: y_domain,
            out_of_domain: chart
                .y
                .domain
                .map(|_| vizir_core::NumericOutOfDomain::Reject),
            range: [plot[3], plot[1]],
            range_space: DOCUMENT_SPACE.to_owned(),
            zero: false,
        },
    ];
    if let Some(scale) = color_scale {
        scales.push(scale);
    }

    Ok(MirChart {
        id: chart.id.clone(),
        title: chart.title.clone(),
        frame: chart.frame,
        space: DOCUMENT_SPACE.to_owned(),
        source,
        row_variable,
        key_expression,
        scales,
        guides: chart_guides(chart, chart.color.as_ref()),
        mark,
        provenance: vec![
            if chart.x.domain.is_some() {
                "x domain authored exactly; outliers rejected".into()
            } else {
                format!("x domain inferred from {} finite values", x_values.len())
            },
            if chart.y.domain.is_some() {
                "y domain authored exactly; outliers rejected".into()
            } else {
                format!("y domain inferred from {} finite values", y_values.len())
            },
            if chart.x.domain.is_some() || chart.y.domain.is_some() {
                "authored numeric bounds retained; only omitted domains use deterministic inference"
                    .into()
            } else {
                "linear domains expanded with deterministic nice-domain policy".to_owned()
            },
            "axes made explicit during chart dialect lowering".to_owned(),
        ],
    })
}

fn lower_line(
    document: &Document,
    chart: &LineChart,
    data: &BTreeMap<String, MirDataNode>,
    expressions: &mut BTreeMap<String, TypedExpression>,
    budget: &mut Budget,
    defaults: Option<&ResolvedThemeDefaults>,
    text: Option<&TextSession>,
) -> Result<MirChart, String> {
    let dataset = dataset(document, &chart.dataset)?;
    let source = data_id(&chart.dataset);
    preflight_hir_bindings(&data[&source], budget).map_err(|e| e.to_string())?;
    let row_variable = row_variable(&chart.id);
    let key_expression = register_field_expression(
        expressions,
        data,
        &source,
        &row_variable,
        &chart.id,
        "key",
        &dataset.key,
    )?;
    let x_expression = register_field_expression(
        expressions,
        data,
        &source,
        &row_variable,
        &chart.id,
        "x",
        &chart.x.field,
    )?;
    let y_expression = register_field_expression(
        expressions,
        data,
        &source,
        &row_variable,
        &chart.id,
        "y",
        &chart.y.field,
    )?;
    let series_expression = chart
        .series
        .as_ref()
        .map(|encoding| {
            register_field_expression(
                expressions,
                data,
                &source,
                &row_variable,
                &chart.id,
                "series",
                &encoding.field,
            )
        })
        .transpose()?;
    let plan = ChartMark::Line {
        id: format!("{}/marks/lines", chart.id),
        x: scale_binding(format!("{}/x", chart.id), x_expression.clone()),
        y: scale_binding(format!("{}/y", chart.id), y_expression),
        color: series_expression
            .clone()
            .map(|expression| scale_binding(format!("{}/color", chart.id), expression)),
        group_expression: series_expression,
        order_expression: x_expression,
        line_width: chart.line_width,
        show_points: chart.show_points,
        series: Vec::new(),
    };
    let mark = materialize_mark(
        &data[&source],
        &row_variable,
        &key_expression,
        &plan,
        expressions,
        &chart.id,
        budget,
    )
    .map_err(|e| e.to_string())?;
    let ChartMark::Line { series, .. } = &mark else {
        unreachable!()
    };
    let x_values = series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.x))
        .collect::<Vec<_>>();
    let y_values = series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.y))
        .collect::<Vec<_>>();
    let x_domain = chart
        .x
        .domain
        .unwrap_or_else(|| nice_domain(extent(&x_values), false));
    let y_domain = chart
        .y
        .domain
        .unwrap_or_else(|| nice_domain(extent(&y_values), false));
    let color_scale = materialized_color_scale(
        &chart.id,
        chart.series.as_ref(),
        series.iter().filter_map(|s| s.color_category.as_ref()),
        defaults,
    );
    let ticks = NumericTickLabels::new_with_measurement(
        Some((x_domain, chart.x.number_format())),
        Some((y_domain, chart.y.number_format())),
        text.is_some(),
        [chart.x.domain.is_some(), chart.y.domain.is_some()],
    )?;
    let plot = ChartLayout::new_with_text(
        &chart.id,
        chart.frame,
        chart.title.as_deref(),
        Some(chart.x.label.as_deref().unwrap_or(&chart.x.field)),
        Some(chart.y.label.as_deref().unwrap_or(&chart.y.field)),
        crate::shared_legend::local_domain(document, &chart.id, color_scale.as_ref()),
        text,
    )?
    .with_numeric_ticks_and_text(
        &chart.id,
        chart.frame,
        Some(chart.x.label.as_deref().unwrap_or(&chart.x.field)),
        ticks.as_ref(),
        text,
    )?
    .plot;
    let mut scales = vec![
        MirScale::Linear {
            id: format!("{}/x", chart.id),
            domain: x_domain,
            out_of_domain: chart
                .x
                .domain
                .map(|_| vizir_core::NumericOutOfDomain::Reject),
            range: [plot[0], plot[2]],
            range_space: DOCUMENT_SPACE.to_owned(),
            zero: false,
        },
        MirScale::Linear {
            id: format!("{}/y", chart.id),
            domain: y_domain,
            out_of_domain: chart
                .y
                .domain
                .map(|_| vizir_core::NumericOutOfDomain::Reject),
            range: [plot[3], plot[1]],
            range_space: DOCUMENT_SPACE.to_owned(),
            zero: false,
        },
    ];
    if let Some(scale) = color_scale {
        scales.push(scale);
    }

    Ok(MirChart {
        id: chart.id.clone(),
        title: chart.title.clone(),
        frame: chart.frame,
        space: DOCUMENT_SPACE.to_owned(),
        source,
        row_variable,
        key_expression,
        scales,
        guides: vec![
            MirGuide {
                id: format!("{}/guides/x-axis", chart.id),
                kind: GuideKind::Axis,
                scale: format!("{}/x", chart.id),
                label: chart
                    .x
                    .label
                    .clone()
                    .unwrap_or_else(|| chart.x.field.clone()),
                orient: GuideOrient::Bottom,
                number_format: chart.x.number_format().copied(),
            },
            MirGuide {
                id: format!("{}/guides/y-axis", chart.id),
                kind: GuideKind::Axis,
                scale: format!("{}/y", chart.id),
                label: chart
                    .y
                    .label
                    .clone()
                    .unwrap_or_else(|| chart.y.field.clone()),
                orient: GuideOrient::Left,
                number_format: chart.y.number_format().copied(),
            },
        ],
        mark,
        provenance: vec![
            "rows grouped by stable series value and sorted by x encoding".to_owned(),
            if chart.x.domain.is_some() || chart.y.domain.is_some() {
                "authored numeric bounds retained exactly; outliers rejected; omitted domains inferred".into()
            } else {
                "linear scale domains inferred and expanded deterministically".to_owned()
            },
            "line topology remains in MIR; coordinates are unresolved".to_owned(),
        ],
    })
}

fn lower_area(
    document: &Document,
    chart: &AreaChart,
    data: &BTreeMap<String, MirDataNode>,
    expressions: &mut BTreeMap<String, TypedExpression>,
    budget: &mut Budget,
    defaults: Option<&ResolvedThemeDefaults>,
    text: Option<&TextSession>,
) -> Result<MirChart, String> {
    let dataset = dataset(document, &chart.dataset)?;
    let source = data_id(&chart.dataset);
    preflight_hir_bindings(&data[&source], budget).map_err(|e| e.to_string())?;
    let row_variable = row_variable(&chart.id);
    let key_expression = register_field_expression(
        expressions,
        data,
        &source,
        &row_variable,
        &chart.id,
        "key",
        &dataset.key,
    )?;
    let x_expression = register_field_expression(
        expressions,
        data,
        &source,
        &row_variable,
        &chart.id,
        "x",
        &chart.x.field,
    )?;
    let y_expression = register_field_expression(
        expressions,
        data,
        &source,
        &row_variable,
        &chart.id,
        "y",
        &chart.y.field,
    )?;
    let series_expression = chart
        .series
        .as_ref()
        .map(|encoding| {
            register_field_expression(
                expressions,
                data,
                &source,
                &row_variable,
                &chart.id,
                "series",
                &encoding.field,
            )
        })
        .transpose()?;
    let plan = ChartMark::Area {
        id: format!("{}/marks/areas", chart.id),
        x: scale_binding(format!("{}/x", chart.id), x_expression.clone()),
        y: scale_binding(format!("{}/y", chart.id), y_expression),
        color: series_expression
            .clone()
            .map(|expression| scale_binding(format!("{}/color", chart.id), expression)),
        group_expression: series_expression,
        order_expression: x_expression,
        baseline: chart.baseline,
        series: Vec::new(),
    };
    let mark = materialize_mark(
        &data[&source],
        &row_variable,
        &key_expression,
        &plan,
        expressions,
        &chart.id,
        budget,
    )
    .map_err(|e| e.to_string())?;
    let ChartMark::Area { series, .. } = &mark else {
        unreachable!()
    };
    let x_values = series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.x))
        .collect::<Vec<_>>();
    let y_values = series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.y))
        .collect::<Vec<_>>();
    let x_domain = chart
        .x
        .domain
        .unwrap_or_else(|| nice_domain(extent(&x_values), false));
    let raw_y = extent(&y_values);
    let y_domain = chart.y.domain.unwrap_or_else(|| {
        nice_domain(
            [raw_y[0].min(chart.baseline), raw_y[1].max(chart.baseline)],
            false,
        )
    });
    let color_scale = materialized_color_scale(
        &chart.id,
        chart.series.as_ref(),
        series.iter().filter_map(|s| s.color_category.as_ref()),
        defaults,
    );
    let ticks = NumericTickLabels::new_with_measurement(
        Some((x_domain, chart.x.number_format())),
        Some((y_domain, chart.y.number_format())),
        text.is_some(),
        [chart.x.domain.is_some(), chart.y.domain.is_some()],
    )?;
    let plot = ChartLayout::new_with_text(
        &chart.id,
        chart.frame,
        chart.title.as_deref(),
        Some(chart.x.label.as_deref().unwrap_or(&chart.x.field)),
        Some(chart.y.label.as_deref().unwrap_or(&chart.y.field)),
        crate::shared_legend::local_domain(document, &chart.id, color_scale.as_ref()),
        text,
    )?
    .with_numeric_ticks_and_text(
        &chart.id,
        chart.frame,
        Some(chart.x.label.as_deref().unwrap_or(&chart.x.field)),
        ticks.as_ref(),
        text,
    )?
    .plot;
    let mut scales = vec![
        MirScale::Linear {
            id: format!("{}/x", chart.id),
            domain: x_domain,
            out_of_domain: chart
                .x
                .domain
                .map(|_| vizir_core::NumericOutOfDomain::Reject),
            range: [plot[0], plot[2]],
            range_space: DOCUMENT_SPACE.to_owned(),
            zero: false,
        },
        MirScale::Linear {
            id: format!("{}/y", chart.id),
            domain: y_domain,
            out_of_domain: chart
                .y
                .domain
                .map(|_| vizir_core::NumericOutOfDomain::Reject),
            range: [plot[3], plot[1]],
            range_space: DOCUMENT_SPACE.to_owned(),
            zero: false,
        },
    ];
    if let Some(scale) = color_scale {
        scales.push(scale);
    }

    let mut guides = vec![
        MirGuide {
            id: format!("{}/guides/x-axis", chart.id),
            kind: GuideKind::Axis,
            scale: format!("{}/x", chart.id),
            label: chart
                .x
                .label
                .clone()
                .unwrap_or_else(|| chart.x.field.clone()),
            orient: GuideOrient::Bottom,
            number_format: chart.x.number_format().copied(),
        },
        MirGuide {
            id: format!("{}/guides/y-axis", chart.id),
            kind: GuideKind::Axis,
            scale: format!("{}/y", chart.id),
            label: chart
                .y
                .label
                .clone()
                .unwrap_or_else(|| chart.y.field.clone()),
            orient: GuideOrient::Left,
            number_format: chart.y.number_format().copied(),
        },
    ];
    if let Some(series) = &chart.series {
        guides.push(MirGuide {
            id: format!("{}/guides/color-legend", chart.id),
            kind: GuideKind::Legend,
            scale: format!("{}/color", chart.id),
            label: series.field.clone(),
            orient: GuideOrient::Right,
            number_format: None,
        });
    }
    let lowered = MirChart {
        id: chart.id.clone(),
        title: chart.title.clone(),
        frame: chart.frame,
        space: DOCUMENT_SPACE.to_owned(),
        source,
        row_variable,
        key_expression,
        scales,
        guides,
        mark,
        provenance: vec![
            "source rows and stable keys retained; area groups painted in sorted series-key order".to_owned(),
            if chart.x.domain.is_some() || chart.y.domain.is_some() { "authored numeric bounds retained exactly; observations and baseline must be contained; omitted domains inferred".into() } else { "linear x domain inferred; y domain includes the explicit baseline and all observed values".to_owned() },
            "unstacked linear areas require at least two strictly increasing representable x values per series".to_owned(),
        ],
    };
    crate::materialize::check_area_projection(&lowered, &lowered.mark, budget)
        .map_err(|e| e.to_string())?;
    Ok(lowered)
}

fn lower_bar(
    document: &Document,
    chart: &BarChart,
    data: &BTreeMap<String, MirDataNode>,
    expressions: &mut BTreeMap<String, TypedExpression>,
    budget: &mut Budget,
    defaults: Option<&ResolvedThemeDefaults>,
    text: Option<&TextSession>,
) -> Result<MirChart, String> {
    let dataset = dataset(document, &chart.dataset)?;
    let source = data_id(&chart.dataset);
    preflight_hir_bindings(&data[&source], budget).map_err(|e| e.to_string())?;
    let row_variable = row_variable(&chart.id);
    let key_expression = register_field_expression(
        expressions,
        data,
        &source,
        &row_variable,
        &chart.id,
        "key",
        &dataset.key,
    )?;
    let category_expression = register_field_expression(
        expressions,
        data,
        &source,
        &row_variable,
        &chart.id,
        "category",
        &chart.category.field,
    )?;
    let value_expression = register_field_expression(
        expressions,
        data,
        &source,
        &row_variable,
        &chart.id,
        "value",
        &chart.value.field,
    )?;
    let color_expression = chart
        .color
        .as_ref()
        .map(|encoding| {
            register_field_expression(
                expressions,
                data,
                &source,
                &row_variable,
                &chart.id,
                "color",
                &encoding.field,
            )
        })
        .transpose()?;
    let plan = ChartMark::Bar {
        id: format!("{}/marks/bars", chart.id),
        category: scale_binding(format!("{}/category", chart.id), category_expression),
        value: scale_binding(format!("{}/value", chart.id), value_expression),
        color: color_expression
            .map(|expression| scale_binding(format!("{}/color", chart.id), expression)),
        instances: Vec::new(),
    };
    let mark = materialize_mark(
        &data[&source],
        &row_variable,
        &key_expression,
        &plan,
        expressions,
        &chart.id,
        budget,
    )
    .map_err(|e| e.to_string())?;
    let ChartMark::Bar { instances, .. } = &mark else {
        unreachable!()
    };
    let values = instances.iter().map(|p| p.value).collect::<Vec<_>>();
    let categories: Vec<String> = instances.iter().map(|p| p.category.clone()).collect();
    let raw = extent(&values);
    let domain = chart
        .value
        .domain
        .unwrap_or_else(|| nice_domain([raw[0].min(0.0), raw[1].max(0.0)], true));
    let ticks = NumericTickLabels::new_with_measurement(
        None,
        Some((domain, chart.value.number_format())),
        text.is_some(),
        [false, chart.value.domain.is_some()],
    )?;
    let color_scale = materialized_color_scale(
        &chart.id,
        chart.color.as_ref(),
        instances.iter().filter_map(|p| p.color_category.as_ref()),
        defaults,
    );
    let plot = ChartLayout::new_with_text(
        &chart.id,
        chart.frame,
        chart.title.as_deref(),
        Some(
            chart
                .category
                .label
                .as_deref()
                .unwrap_or(&chart.category.field),
        ),
        Some(chart.value.label.as_deref().unwrap_or(&chart.value.field)),
        crate::shared_legend::local_domain(document, &chart.id, color_scale.as_ref()),
        text,
    )?
    .with_numeric_ticks_and_text(
        &chart.id,
        chart.frame,
        Some(
            chart
                .category
                .label
                .as_deref()
                .unwrap_or(&chart.category.field),
        ),
        ticks.as_ref(),
        text,
    )?
    .with_categories(
        &chart.id,
        chart.frame,
        &categories,
        Some(
            chart
                .category
                .label
                .as_deref()
                .unwrap_or(&chart.category.field),
        ),
        ticks.as_ref(),
        text,
        None,
    )?
    .plot;
    let mut scales = vec![
        MirScale::Band {
            id: format!("{}/category", chart.id),
            domain: categories,
            range: [plot[0], plot[2]],
            range_space: DOCUMENT_SPACE.to_owned(),
            padding: 0.22,
        },
        MirScale::Linear {
            id: format!("{}/value", chart.id),
            domain,
            out_of_domain: chart
                .value
                .domain
                .map(|_| vizir_core::NumericOutOfDomain::Reject),
            range: [plot[3], plot[1]],
            range_space: DOCUMENT_SPACE.to_owned(),
            zero: true,
        },
    ];
    if let Some(scale) = color_scale {
        scales.push(scale);
    }

    Ok(MirChart {
        id: chart.id.clone(),
        title: chart.title.clone(),
        frame: chart.frame,
        space: DOCUMENT_SPACE.to_owned(),
        source,
        row_variable,
        key_expression,
        scales,
        guides: vec![
            MirGuide {
                id: format!("{}/guides/category-axis", chart.id),
                kind: GuideKind::Axis,
                scale: format!("{}/category", chart.id),
                label: chart
                    .category
                    .label
                    .clone()
                    .unwrap_or_else(|| chart.category.field.clone()),
                orient: GuideOrient::Bottom,
                number_format: chart.category.number_format().copied(),
            },
            MirGuide {
                id: format!("{}/guides/value-axis", chart.id),
                kind: GuideKind::Axis,
                scale: format!("{}/value", chart.id),
                label: chart
                    .value
                    .label
                    .clone()
                    .unwrap_or_else(|| chart.value.field.clone()),
                orient: GuideOrient::Left,
                number_format: chart.value.number_format().copied(),
            },
        ],
        mark,
        provenance: vec![
            "one bar generated per unique category".to_owned(),
            if chart.value.domain.is_some() {
                "authored value domain retained exactly and includes zero; outliers rejected".into()
            } else {
                "quantitative domain includes zero to preserve bar-chart truth".to_owned()
            },
            "band placement remains unresolved until Scene2D construction".to_owned(),
        ],
    })
}

#[allow(clippy::too_many_arguments)]
fn lower_heatmap(
    document: &Document,
    chart: &HeatmapChart,
    data: &BTreeMap<String, MirDataNode>,
    expressions: &mut BTreeMap<String, TypedExpression>,
    budget: &mut Budget,
    defaults: Option<&ResolvedThemeDefaults>,
    text: Option<&TextSession>,
) -> Result<MirChart, String> {
    let dataset = dataset(document, &chart.dataset)?;
    let source = data_id(&chart.dataset);
    preflight_hir_bindings(&data[&source], budget).map_err(|e| e.to_string())?;
    let row_variable = row_variable(&chart.id);
    let mut field = |role: &str, name: &str| {
        register_field_expression(
            expressions,
            data,
            &source,
            &row_variable,
            &chart.id,
            role,
            name,
        )
    };
    let key_expression = field("key", &dataset.key)?;
    let x = scale_binding(format!("{}/x", chart.id), field("x", &chart.x.field)?);
    let y = scale_binding(format!("{}/y", chart.id), field("y", &chart.y.field)?);
    let color = scale_binding(
        format!("{}/color", chart.id),
        field("color", &chart.color.field)?,
    );
    let plan = ChartMark::Heatmap {
        id: format!("{}/marks/cells", chart.id),
        x,
        y,
        color,
        instances: Vec::new(),
        value_labels: chart.value_labels.as_ref().map(|options| {
            vizir_core::MirHeatmapValueLabels {
                number_format: options.number_format,
                color: options.color.clone(),
                instances: Vec::new(),
            }
        }),
    };
    let mark = materialize_mark(
        &data[&source],
        &row_variable,
        &key_expression,
        &plan,
        expressions,
        &chart.id,
        budget,
    )
    .map_err(|e| e.to_string())?;
    let ChartMark::Heatmap { instances, .. } = &mark else {
        unreachable!()
    };
    let x_domain = crate::heatmap::resolve_domain(
        chart.x.domain.as_deref(),
        instances.iter().map(|p| p.x.as_str()),
    )
    .map_err(|e| e.to_string())?;
    let y_domain = crate::heatmap::resolve_domain(
        chart.y.domain.as_deref(),
        instances.iter().map(|p| p.y.as_str()),
    )
    .map_err(|e| e.to_string())?;
    crate::heatmap::check_domains(&x_domain, &y_domain).map_err(|e| e.to_string())?;
    let mut domain = chart.color.domain.unwrap_or_else(|| {
        instances
            .iter()
            .fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], p| {
                [lo.min(p.value), hi.max(p.value)]
            })
    });
    // Preserve all original source values; only resolved zero endpoints canonicalize.
    for endpoint in &mut domain {
        if *endpoint == 0. {
            *endpoint = 0.;
        }
    }
    let range = chart
        .color
        .palette
        .clone()
        .map(Ok)
        .unwrap_or_else(|| crate::heatmap::palette(defaults))
        .map_err(|e| e.to_string())?;
    let thresholds = vizir_core::canonical_quantize_thresholds(domain, range.len())
        .map_err(|e| e.to_string())?;
    let color_scale = MirScale::QuantizeColor {
        id: format!("{}/color", chart.id),
        domain,
        thresholds,
        range,
    };
    let legend =
        crate::heatmap::IntervalLegend::new(&color_scale, chart.color.number_format.as_ref())
            .map_err(|e| e.to_string())?;
    let x_label = chart.x.label.as_deref().unwrap_or(&chart.x.field);
    let y_label = chart.y.label.as_deref().unwrap_or(&chart.y.field);
    let color_label = chart.color.label.as_deref().unwrap_or(&chart.color.field);
    let layout_labels = x_domain.len() + y_domain.len() + legend.labels.len() * 2 + 4;
    budget
        .charge((layout_labels * layout_labels) as u64, &chart.id)
        .map_err(|e| e.to_string())?;
    let plot = crate::heatmap::HeatmapLayout::new(
        &chart.id,
        chart.frame,
        chart.title.as_deref(),
        &x_domain,
        &y_domain,
        x_label,
        y_label,
        color_label,
        &legend,
        text,
    )
    .map_err(|e| e.to_string())?
    .plot;
    let result = MirChart {
        id: chart.id.clone(), title: chart.title.clone(), frame: chart.frame,
        space: DOCUMENT_SPACE.to_owned(), source, row_variable, key_expression,
        scales: vec![
            MirScale::Band { id: format!("{}/x", chart.id), domain: x_domain,
            range: [plot[0],plot[2]], range_space: DOCUMENT_SPACE.to_owned(), padding: 0. },
            MirScale::Band { id: format!("{}/y", chart.id), domain: y_domain,
            range: [plot[1],plot[3]], range_space: DOCUMENT_SPACE.to_owned(), padding: 0. },
            color_scale,
        ],
        guides: vec![
            MirGuide { id: format!("{}/guides/x-axis", chart.id), kind: GuideKind::Axis, scale: format!("{}/x", chart.id), label: x_label.into(), orient: GuideOrient::Bottom, number_format: None },
            MirGuide { id: format!("{}/guides/y-axis", chart.id), kind: GuideKind::Axis, scale: format!("{}/y", chart.id), label: y_label.into(), orient: GuideOrient::Left, number_format: None },
            MirGuide { id: format!("{}/guides/color-legend", chart.id), kind: GuideKind::Legend, scale: format!("{}/color", chart.id), label: color_label.into(), orient: GuideOrient::Right, number_format: chart.color.number_format },
        ],
        mark,
        provenance: vec![
            "sparse heatmap: one keyed cell per unique string (x,y) pair; source rows and cache order retained".into(),
            "x categories run left to right; y categories run top to bottom; absent pairs create no cell".into(),
            "quantize-ramp/1: explicit numeric intervals, upper-bin ties, closed final endpoint, strict domain membership".into(),
            "resolved full palette retained; constant domains select the lower middle color".into(),
        ],
    };
    crate::materialize::check_scale_membership(&result, &result.mark, budget)
        .map_err(|e| e.to_string())?;
    Ok(result)
}

fn chart_guides(chart: &ScatterChart, color: Option<&ColorEncoding>) -> Vec<MirGuide> {
    let mut guides = vec![
        MirGuide {
            id: format!("{}/guides/x-axis", chart.id),
            kind: GuideKind::Axis,
            scale: format!("{}/x", chart.id),
            label: chart
                .x
                .label
                .clone()
                .unwrap_or_else(|| chart.x.field.clone()),
            orient: GuideOrient::Bottom,
            number_format: chart.x.number_format().copied(),
        },
        MirGuide {
            id: format!("{}/guides/y-axis", chart.id),
            kind: GuideKind::Axis,
            scale: format!("{}/y", chart.id),
            label: chart
                .y
                .label
                .clone()
                .unwrap_or_else(|| chart.y.field.clone()),
            orient: GuideOrient::Left,
            number_format: chart.y.number_format().copied(),
        },
    ];
    if let Some(color) = color {
        guides.push(MirGuide {
            id: format!("{}/guides/color-legend", chart.id),
            kind: GuideKind::Legend,
            scale: format!("{}/color", chart.id),
            label: color.field.clone(),
            orient: GuideOrient::Right,
            number_format: None,
        });
    }
    guides
}

fn materialized_color_scale<'a>(
    chart_id: &str,
    encoding: Option<&ColorEncoding>,
    categories: impl Iterator<Item = &'a String>,
    defaults: Option<&ResolvedThemeDefaults>,
) -> Option<MirScale> {
    encoding.map(|encoding| {
        let domain = encoding.domain.clone().unwrap_or_else(|| {
            categories
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        });
        let palette = if encoding.palette.is_empty() {
            defaults.map(|d| d.series.to_vec()).unwrap_or_else(|| {
                DEFAULT_PALETTE
                    .iter()
                    .map(|color| Color::hex(color))
                    .collect()
            })
        } else {
            encoding.palette.clone()
        };
        let range = (0..domain.len())
            .map(|index| palette[index % palette.len()].clone())
            .collect();
        MirScale::OrdinalColor {
            id: format!("{chart_id}/color"),
            domain,
            range,
        }
    })
}

fn lower_data(document: &Document) -> Result<BTreeMap<String, MirDataNode>, String> {
    document
        .datasets
        .iter()
        .map(|(name, dataset)| {
            let id = data_id(name);
            let fields = infer_dataset_fields(&dataset.rows)?;
            Ok((
                id.clone(),
                MirDataNode {
                    id,
                    schema: MirDataSchema {
                        key: dataset.key.clone(),
                        fields,
                    },
                    operator: MirDataOperator::Inline {
                        rows: dataset.rows.clone(),
                    },
                    update_mode: UpdateMode::Replace,
                    deterministic: true,
                },
            ))
        })
        .collect()
}

fn infer_dataset_fields(
    rows: &[BTreeMap<String, Value>],
) -> Result<BTreeMap<String, ValueType>, String> {
    let names = rows
        .iter()
        .flat_map(|row| row.keys().cloned())
        .collect::<BTreeSet<_>>();
    names
        .into_iter()
        .map(|name| {
            let mut values = rows.iter().map(|row| {
                row.get(&name)
                    .map(value_type)
                    .transpose()
                    .map(|value| value.unwrap_or(ValueType::Null))
            });
            let mut field_type = values.next().transpose()?.unwrap_or(ValueType::Null);
            for next in values {
                let next = next?;
                field_type = merge_data_types(&field_type, &next).ok_or_else(|| {
                    format!(
                        "field {name:?} has incompatible inferred types {field_type:?} and {next:?}"
                    )
                })?;
            }
            Ok((name, field_type))
        })
        .collect()
}

fn value_type(value: &Value) -> Result<ValueType, String> {
    match value {
        Value::Null => Ok(ValueType::Null),
        Value::Bool(_) => Ok(ValueType::Bool),
        Value::Number(number) if number.is_i64() => Ok(ValueType::Int64),
        Value::Number(_) => Ok(ValueType::Float64),
        Value::String(_) => Ok(ValueType::String),
        Value::Array(items) => {
            let mut item_type = ValueType::Null;
            for item in items {
                let next = value_type(item)?;
                item_type = merge_data_types(&item_type, &next).ok_or_else(|| {
                    format!("array contains incompatible types {item_type:?} and {next:?}")
                })?;
            }
            Ok(ValueType::Array {
                items: Box::new(item_type),
            })
        }
        Value::Object(fields) => fields
            .iter()
            .map(|(name, value)| Ok((name.clone(), value_type(value)?)))
            .collect::<Result<BTreeMap<_, _>, String>>()
            .map(|fields| ValueType::Record { fields }),
    }
}

fn merge_data_types(left: &ValueType, right: &ValueType) -> Option<ValueType> {
    if left == right {
        return Some(left.clone());
    }
    if matches!(left, ValueType::Int64 | ValueType::Float64)
        && matches!(right, ValueType::Int64 | ValueType::Float64)
    {
        return Some(ValueType::Float64);
    }
    match (left, right) {
        (ValueType::Null, ValueType::Null) => Some(ValueType::Null),
        (ValueType::Option { item }, ValueType::Null)
        | (ValueType::Null, ValueType::Option { item }) => {
            Some(ValueType::option(item.as_ref().clone()))
        }
        (ValueType::Null, value) | (value, ValueType::Null) => {
            Some(ValueType::option(value.clone()))
        }
        (ValueType::Option { item }, value) | (value, ValueType::Option { item }) => {
            merge_data_types(item, value).map(ValueType::option)
        }
        (ValueType::Array { items: left }, ValueType::Array { items: right }) => {
            merge_data_types(left, right).map(|items| ValueType::Array {
                items: Box::new(items),
            })
        }
        (ValueType::Record { fields: left }, ValueType::Record { fields: right })
            if left.keys().eq(right.keys()) =>
        {
            left.iter()
                .map(|(name, left_type)| {
                    merge_data_types(left_type, &right[name])
                        .map(|value_type| (name.clone(), value_type))
                })
                .collect::<Option<BTreeMap<_, _>>>()
                .map(|fields| ValueType::Record { fields })
        }
        _ => None,
    }
}

fn register_field_expression(
    expressions: &mut BTreeMap<String, TypedExpression>,
    data: &BTreeMap<String, MirDataNode>,
    source: &str,
    row_variable: &str,
    chart_id: &str,
    channel: &str,
    field: &str,
) -> Result<String, String> {
    let data_node = data
        .get(source)
        .ok_or_else(|| format!("unknown normalized data source {source:?}"))?;
    let environment = TypeEnvironment {
        rows: BTreeMap::from([(row_variable.to_owned(), data_node.schema.fields.clone())]),
        ..TypeEnvironment::default()
    };
    let typed = type_expression(
        Expression::Field {
            row: row_variable.to_owned(),
            field: field.to_owned(),
        },
        &environment,
    )
    .map_err(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))?;
    let id = format!("expr/{chart_id}/{channel}");
    if expressions.insert(id.clone(), typed).is_some() {
        return Err(format!("duplicate expression id {id:?}"));
    }
    Ok(id)
}

fn scale_binding(scale: String, expression: String) -> ScaleBinding {
    ScaleBinding { scale, expression }
}

fn data_id(name: &str) -> String {
    format!("data/{name}")
}

fn row_variable(chart_id: &str) -> String {
    format!("row/{chart_id}")
}

fn lower_geometry_node(
    node: &GeometryNode,
    defaults: Option<&ResolvedThemeDefaults>,
) -> MirGeometryNode {
    match node {
        GeometryNode::Group {
            id,
            transform,
            opacity,
            children,
        } => MirGeometryNode::Group {
            id: id.clone(),
            transform: *transform,
            opacity: opacity.unwrap_or(1.0),
            children: children
                .iter()
                .map(|node| lower_geometry_node(node, defaults))
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
        } => MirGeometryNode::Rect {
            id: id.clone(),
            x: *x,
            y: *y,
            width: *width,
            height: *height,
            radius: *radius,
            style: lower_style(style, Color::transparent(), Color::transparent()),
        },
        GeometryNode::Circle {
            id,
            cx,
            cy,
            radius,
            style,
        } => MirGeometryNode::Circle {
            id: id.clone(),
            cx: *cx,
            cy: *cy,
            radius: *radius,
            style: lower_style(style, Color::transparent(), Color::transparent()),
        },
        GeometryNode::Line {
            id,
            from,
            to,
            style,
        } => MirGeometryNode::Line {
            id: id.clone(),
            from: *from,
            to: *to,
            style: lower_style(
                style,
                Color::transparent(),
                defaults
                    .map(|d| d.ink.clone())
                    .unwrap_or_else(|| Color::hex("#1C2736")),
            ),
        },
        GeometryNode::Path {
            id,
            commands,
            style,
        } => MirGeometryNode::Path {
            id: id.clone(),
            commands: commands.clone(),
            style: lower_style(
                style,
                Color::transparent(),
                defaults
                    .map(|d| d.ink.clone())
                    .unwrap_or_else(|| Color::hex("#1C2736")),
            ),
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
        } => MirGeometryNode::Text {
            id: id.clone(),
            x: *x,
            y: *y,
            text: text.clone(),
            font_size: *font_size,
            anchor: *anchor,
            color: color.clone().unwrap_or_else(|| {
                defaults
                    .map(|d| d.ink.clone())
                    .unwrap_or_else(|| Color::hex("#1C2736"))
            }),
            weight: *weight,
        },
    }
}

fn lower_style(
    style: &vizir_core::ShapeStyle,
    default_fill: Color,
    default_stroke: Color,
) -> MirShapeStyle {
    MirShapeStyle {
        fill: style.fill.clone().unwrap_or(default_fill),
        stroke: style.stroke.clone().unwrap_or(default_stroke),
        stroke_width: style.stroke_width,
        opacity: style.opacity,
    }
}

fn dataset<'a>(document: &'a Document, name: &str) -> Result<&'a vizir_core::Dataset, String> {
    document
        .datasets
        .get(name)
        .ok_or_else(|| format!("dataset {name:?} disappeared after validation"))
}

fn extent(values: &[f64]) -> [f64; 2] {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for value in values {
        min = min.min(*value);
        max = max.max(*value);
    }
    [min, max]
}

fn nice_domain(domain: [f64; 2], include_zero: bool) -> [f64; 2] {
    let mut min = domain[0];
    let mut max = domain[1];
    if include_zero {
        min = min.min(0.0);
        max = max.max(0.0);
    }
    // A small representable span is still a meaningful nonconstant domain.
    if min == max {
        let delta = max.abs().max(1.0) * 0.1;
        return [min - delta, max + delta];
    }
    let span = max - min;
    let power = 10_f64.powf(span.log10().floor());
    let normalized = span / power;
    let step = if normalized < 2.0 {
        power / 5.0
    } else if normalized < 5.0 {
        power / 2.0
    } else {
        power
    };
    // At subnormal magnitudes the decimal power or its subdivision can
    // underflow to zero. Keep the finite input extent instead of dividing
    // by that unrepresentable step and manufacturing NaN bounds.
    if step == 0.0 {
        return [min, max];
    }
    // Multiplying a rounded quotient can land one rounding unit inside
    // an original endpoint, so never shrink past the input extent.
    [
        ((min / step).floor() * step).min(min),
        ((max / step).ceil() * step).max(max),
    ]
}

fn lowering_error(message: String) -> VizError {
    VizError::Diagnostic(format!("VIZ-LOWER-0001: {message}"))
}

#[cfg(test)]
#[path = "lower_tests.rs"]
mod tests;
