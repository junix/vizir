use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use vizir_core::{
    BarChart, ChartMark, Color, ColorEncoding, CoordinateSpace2D, CoordinateSpaceKind, Document,
    Expression, GeometryNode, GuideKind, GuideOrient, LineChart, MirBarItem, MirChart, MirDataNode,
    MirDataOperator, MirDataSchema, MirDiagram, MirGeometry, MirGeometryNode, MirGuide,
    MirPointItem, MirScale, MirSeries, MirShapeStyle, MirView, ScaleBinding, ScatterChart,
    SpatialUnit, Transform2D, TypeEnvironment, TypedExpression, UpdateMode, ValueType, View,
    VizError, VizMir, VizResult, type_expression, value_as_key,
};

const DEFAULT_PALETTE: [&str; 8] = [
    "#3B6EF5", "#EB5E55", "#18A999", "#F2A541", "#7A5AF8", "#D94891", "#4B8B3B", "#65758B",
];

const DOCUMENT_SPACE: &str = "space/document";

pub fn lower_to_mir(document: &Document) -> VizResult<VizMir> {
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
                lower_scatter(document, chart, &data, &mut expressions).map_err(lowering_error)?,
            )),
            View::Line(chart) => MirView::Chart(Box::new(
                lower_line(document, chart, &data, &mut expressions).map_err(lowering_error)?,
            )),
            View::Bar(chart) => MirView::Chart(Box::new(
                lower_bar(document, chart, &data, &mut expressions).map_err(lowering_error)?,
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
                    children: geometry.children.iter().map(lower_geometry_node).collect(),
                    provenance: vec![
                        "geometry.scene defaults expanded into normalized portable primitives"
                            .to_owned(),
                        "view-local coordinates linked explicitly to the document space".to_owned(),
                    ],
                })
            }
        });
    }

    Ok(VizMir {
        version: "0.1".to_owned(),
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
    })
}

fn lower_scatter(
    document: &Document,
    chart: &ScatterChart,
    data: &BTreeMap<String, MirDataNode>,
    expressions: &mut BTreeMap<String, TypedExpression>,
) -> Result<MirChart, String> {
    let dataset = dataset(document, &chart.dataset)?;
    let source = data_id(&chart.dataset);
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
    let plot = chart_plot_bounds(chart.frame);
    let x_values = numeric_values(&dataset.rows, &chart.x.field)?;
    let y_values = numeric_values(&dataset.rows, &chart.y.field)?;
    let x_domain = nice_domain(extent(&x_values), false);
    let y_domain = nice_domain(extent(&y_values), false);
    let color_scale = color_scale(&chart.id, chart.color.as_ref(), &dataset.rows)?;
    let mut items = Vec::with_capacity(dataset.rows.len());
    for row in &dataset.rows {
        items.push(MirPointItem {
            key: key(row, &dataset.key)?,
            x: number(row, &chart.x.field)?,
            y: number(row, &chart.y.field)?,
            color_category: optional_category(row, chart.color.as_ref())?,
        });
    }

    let mut scales = vec![
        MirScale::Linear {
            id: format!("{}/x", chart.id),
            domain: x_domain,
            range: [plot[0], plot[2]],
            range_space: DOCUMENT_SPACE.to_owned(),
            zero: false,
        },
        MirScale::Linear {
            id: format!("{}/y", chart.id),
            domain: y_domain,
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
        mark: ChartMark::Symbol {
            id: format!("{}/marks/points", chart.id),
            x: scale_binding(format!("{}/x", chart.id), x_expression),
            y: scale_binding(format!("{}/y", chart.id), y_expression),
            color: color_expression
                .map(|expression| scale_binding(format!("{}/color", chart.id), expression)),
            size: chart.point_size,
            instances: items,
        },
        provenance: vec![
            format!("x domain inferred from {} finite values", x_values.len()),
            format!("y domain inferred from {} finite values", y_values.len()),
            "linear domains expanded with deterministic nice-domain policy".to_owned(),
            "axes made explicit during chart dialect lowering".to_owned(),
        ],
    })
}

fn lower_line(
    document: &Document,
    chart: &LineChart,
    data: &BTreeMap<String, MirDataNode>,
    expressions: &mut BTreeMap<String, TypedExpression>,
) -> Result<MirChart, String> {
    let dataset = dataset(document, &chart.dataset)?;
    let source = data_id(&chart.dataset);
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
    let plot = chart_plot_bounds(chart.frame);
    let x_values = numeric_values(&dataset.rows, &chart.x.field)?;
    let y_values = numeric_values(&dataset.rows, &chart.y.field)?;
    let x_domain = nice_domain(extent(&x_values), false);
    let y_domain = nice_domain(extent(&y_values), false);
    let color_scale = color_scale(&chart.id, chart.series.as_ref(), &dataset.rows)?;
    let mut grouped: BTreeMap<String, Vec<MirPointItem>> = BTreeMap::new();
    for row in &dataset.rows {
        let series =
            optional_category(row, chart.series.as_ref())?.unwrap_or_else(|| "series".to_owned());
        grouped
            .entry(series.clone())
            .or_default()
            .push(MirPointItem {
                key: key(row, &dataset.key)?,
                x: number(row, &chart.x.field)?,
                y: number(row, &chart.y.field)?,
                color_category: chart.series.as_ref().map(|_| series),
            });
    }
    let mut series = grouped
        .into_iter()
        .map(|(key, mut points)| {
            points.sort_by(|a, b| a.x.total_cmp(&b.x).then_with(|| a.key.cmp(&b.key)));
            MirSeries {
                color_category: chart.series.as_ref().map(|_| key.clone()),
                key,
                points,
            }
        })
        .collect::<Vec<_>>();
    series.sort_by(|a, b| a.key.cmp(&b.key));

    let mut scales = vec![
        MirScale::Linear {
            id: format!("{}/x", chart.id),
            domain: x_domain,
            range: [plot[0], plot[2]],
            range_space: DOCUMENT_SPACE.to_owned(),
            zero: false,
        },
        MirScale::Linear {
            id: format!("{}/y", chart.id),
            domain: y_domain,
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
            },
        ],
        mark: ChartMark::Line {
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
            series,
        },
        provenance: vec![
            "rows grouped by stable series value and sorted by x encoding".to_owned(),
            "linear scale domains inferred and expanded deterministically".to_owned(),
            "line topology remains in MIR; coordinates are unresolved".to_owned(),
        ],
    })
}

fn lower_bar(
    document: &Document,
    chart: &BarChart,
    data: &BTreeMap<String, MirDataNode>,
    expressions: &mut BTreeMap<String, TypedExpression>,
) -> Result<MirChart, String> {
    let dataset = dataset(document, &chart.dataset)?;
    let source = data_id(&chart.dataset);
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
    let plot = chart_plot_bounds(chart.frame);
    let values = numeric_values(&dataset.rows, &chart.value.field)?;
    let categories = dataset
        .rows
        .iter()
        .map(|row| category(row, &chart.category.field))
        .collect::<Result<Vec<_>, _>>()?;
    let mut unique = BTreeSet::new();
    for category in &categories {
        if !unique.insert(category.clone()) {
            return Err(format!("bar category {category:?} is duplicated"));
        }
    }
    let color_scale = color_scale(&chart.id, chart.color.as_ref(), &dataset.rows)?;
    let mut items = Vec::with_capacity(dataset.rows.len());
    for row in &dataset.rows {
        items.push(MirBarItem {
            key: key(row, &dataset.key)?,
            category: category(row, &chart.category.field)?,
            value: number(row, &chart.value.field)?,
            color_category: optional_category(row, chart.color.as_ref())?,
        });
    }
    let raw = extent(&values);
    let domain = nice_domain([raw[0].min(0.0), raw[1].max(0.0)], true);
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
            },
        ],
        mark: ChartMark::Bar {
            id: format!("{}/marks/bars", chart.id),
            category: scale_binding(format!("{}/category", chart.id), category_expression),
            value: scale_binding(format!("{}/value", chart.id), value_expression),
            color: color_expression
                .map(|expression| scale_binding(format!("{}/color", chart.id), expression)),
            instances: items,
        },
        provenance: vec![
            "one bar generated per unique category".to_owned(),
            "quantitative domain includes zero to preserve bar-chart truth".to_owned(),
            "band placement remains unresolved until Scene2D construction".to_owned(),
        ],
    })
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
        },
    ];
    if let Some(color) = color {
        guides.push(MirGuide {
            id: format!("{}/guides/color-legend", chart.id),
            kind: GuideKind::Legend,
            scale: format!("{}/color", chart.id),
            label: color.field.clone(),
            orient: GuideOrient::Right,
        });
    }
    guides
}

fn color_scale(
    chart_id: &str,
    encoding: Option<&ColorEncoding>,
    rows: &[BTreeMap<String, Value>],
) -> Result<Option<MirScale>, String> {
    let Some(encoding) = encoding else {
        return Ok(None);
    };
    let domain = rows
        .iter()
        .map(|row| category(row, &encoding.field))
        .collect::<Result<BTreeSet<_>, _>>()?
        .into_iter()
        .collect::<Vec<_>>();
    let palette = if encoding.palette.is_empty() {
        DEFAULT_PALETTE
            .iter()
            .map(|value| Color::hex(value))
            .collect::<Vec<_>>()
    } else {
        encoding.palette.clone()
    };
    let range = (0..domain.len())
        .map(|index| palette[index % palette.len()].clone())
        .collect();
    Ok(Some(MirScale::OrdinalColor {
        id: format!("{chart_id}/color"),
        domain,
        range,
    }))
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

fn lower_geometry_node(node: &GeometryNode) -> MirGeometryNode {
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
            children: children.iter().map(lower_geometry_node).collect(),
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
            style: lower_style(style, Color::transparent(), Color::hex("#1C2736")),
        },
        GeometryNode::Path {
            id,
            commands,
            style,
        } => MirGeometryNode::Path {
            id: id.clone(),
            commands: commands.clone(),
            style: lower_style(style, Color::transparent(), Color::hex("#1C2736")),
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
            color: color.clone().unwrap_or_else(|| Color::hex("#1C2736")),
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

fn chart_plot_bounds(frame: vizir_core::Frame) -> [f64; 4] {
    [
        frame.x + 64.0,
        frame.y + 50.0,
        frame.x + frame.width - 30.0,
        frame.y + frame.height - 62.0,
    ]
}

fn dataset<'a>(document: &'a Document, name: &str) -> Result<&'a vizir_core::Dataset, String> {
    document
        .datasets
        .get(name)
        .ok_or_else(|| format!("dataset {name:?} disappeared after validation"))
}

fn key(row: &BTreeMap<String, Value>, field: &str) -> Result<String, String> {
    row.get(field)
        .and_then(value_as_key)
        .ok_or_else(|| format!("stable key field {field:?} disappeared after validation"))
}

fn number(row: &BTreeMap<String, Value>, field: &str) -> Result<f64, String> {
    row.get(field)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("numeric field {field:?} disappeared after validation"))
}

fn category(row: &BTreeMap<String, Value>, field: &str) -> Result<String, String> {
    row.get(field)
        .and_then(value_as_key)
        .ok_or_else(|| format!("category field {field:?} disappeared after validation"))
}

fn optional_category(
    row: &BTreeMap<String, Value>,
    encoding: Option<&ColorEncoding>,
) -> Result<Option<String>, String> {
    encoding
        .map(|encoding| category(row, &encoding.field))
        .transpose()
}

fn numeric_values(rows: &[BTreeMap<String, Value>], field: &str) -> Result<Vec<f64>, String> {
    rows.iter().map(|row| number(row, field)).collect()
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
    if (max - min).abs() < f64::EPSILON {
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
    [(min / step).floor() * step, (max / step).ceil() * step]
}

fn lowering_error(message: String) -> VizError {
    VizError::Diagnostic(format!("VIZ-LOWER-0001: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_domain_is_deterministic_and_expands() {
        assert_eq!(nice_domain([46.0, 230.0], false), [40.0, 240.0]);
        assert_eq!(nice_domain([2.0, 9.0], true), [0.0, 9.0]);
    }

    #[test]
    fn nice_domain_covers_each_step_bucket_and_degenerate_domains() {
        // normalized in [2, 5) picks half-power steps; normalized >= 5 keeps
        // whole powers; degenerate spans expand symmetrically; include_zero
        // keeps a negative minimum.
        assert_eq!(nice_domain([12.0, 38.0], false), [10.0, 40.0]);
        assert_eq!(nice_domain([0.3, 6.8], false), [0.0, 7.0]);
        assert_eq!(nice_domain([5.0, 5.0], false), [4.5, 5.5]);
        assert_eq!(nice_domain([0.0, 0.0], false), [-0.1, 0.1]);
        assert_eq!(nice_domain([-3.0, 9.0], true), [-4.0, 10.0]);
    }

    #[test]
    fn nullable_numeric_fields_promote_without_nested_options() {
        let rows = vec![
            BTreeMap::from([("value".to_owned(), Value::Null)]),
            BTreeMap::from([("value".to_owned(), serde_json::json!(1))]),
            BTreeMap::from([("value".to_owned(), serde_json::json!(2.5))]),
        ];
        let fields = infer_dataset_fields(&rows).unwrap();
        assert_eq!(fields["value"], ValueType::option(ValueType::Float64));
    }

    #[test]
    fn infer_dataset_fields_rejects_incompatible_columns_with_exact_messages() {
        let rows = vec![
            BTreeMap::from([("value".to_owned(), serde_json::json!(true))]),
            BTreeMap::from([("value".to_owned(), serde_json::json!(1))]),
        ];
        assert_eq!(
            infer_dataset_fields(&rows).unwrap_err(),
            "field \"value\" has incompatible inferred types Bool and Int64"
        );

        // Incompatible items inside one array are reported against the merged
        // item type, not the raw JSON types.
        let mixed = vec![BTreeMap::from([(
            "mixed".to_owned(),
            serde_json::json!([true, 1]),
        )])];
        assert_eq!(
            infer_dataset_fields(&mixed).unwrap_err(),
            "array contains incompatible types Option { item: Bool } and Int64"
        );
    }

    #[test]
    fn infer_dataset_fields_treats_sparse_columns_as_optional() {
        let rows = vec![
            BTreeMap::from([
                ("a".to_owned(), serde_json::json!(1)),
                ("kept".to_owned(), serde_json::json!("x")),
            ]),
            BTreeMap::from([("b".to_owned(), serde_json::json!(2.5))]),
        ];
        let fields = infer_dataset_fields(&rows).unwrap();
        assert_eq!(
            fields,
            BTreeMap::from([
                ("a".to_owned(), ValueType::option(ValueType::Int64)),
                ("b".to_owned(), ValueType::option(ValueType::Float64)),
                ("kept".to_owned(), ValueType::option(ValueType::String)),
            ])
        );

        // No rows means no inferred fields, and the i64/u64 boundary must not
        // claim Int64 for numbers that overflow it.
        assert!(infer_dataset_fields(&[]).unwrap().is_empty());
        let boundaries = vec![
            BTreeMap::from([("n".to_owned(), serde_json::json!(i64::MAX))]),
            BTreeMap::from([("n".to_owned(), serde_json::json!(u64::MAX))]),
        ];
        let fields = infer_dataset_fields(&boundaries).unwrap();
        assert_eq!(fields["n"], ValueType::Float64);
    }
}
